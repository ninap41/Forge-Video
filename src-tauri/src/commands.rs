//! Thin `#[tauri::command]` layer. Every mutating command returns the full `Project` so the
//! frontend store can replace its state wholesale.

use crate::error::{Error, Result};
use crate::jobs::{JobDone, JobError, JobProgress, EVT_DONE, EVT_ERROR, EVT_PROGRESS};
use crate::ai::{self, Cancel};
use crate::project::{AspectPreset, AudioClip, Clip, Crop, MediaInfo, MediaKind, Ms, OverlayClip, Placement, PoolItem, Project, TextClip, TextStyle, Transcript, Transition};
use crate::render::{ExportPlan, ExportSettings};
use crate::{timeline, AppState};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter, Manager, State};
use uuid::Uuid;

type S<'a> = State<'a, AppState>;

fn with_project<T>(state: &S, f: impl FnOnce(&mut Project) -> Result<T>) -> Result<T> {
    let mut p = state.project.lock().unwrap();
    f(&mut p)
}

fn snapshot(state: &S) -> Project {
    state.project.lock().unwrap().clone()
}

#[tauri::command]
pub fn project_get(state: S) -> Project {
    snapshot(&state)
}

#[tauri::command]
pub fn project_new(state: S, name: String) -> Project {
    *state.project.lock().unwrap() = Project::new(name);
    *state.project_path.lock().unwrap() = None;
    snapshot(&state)
}

/// Open a project and re-probe its pool so files saved with an older probe (e.g. an mp3 whose cover
/// art made it a "still") come back with the right kind. Best effort: unreadable files keep their info.
#[tauri::command]
pub async fn project_open(state: S<'_>, path: PathBuf) -> Result<Project> {
    let mut p = crate::project::persist::load(&path)?;
    refresh_media(&mut p).await;
    *state.project.lock().unwrap() = p;
    *state.project_path.lock().unwrap() = Some(path);
    Ok(snapshot(&state))
}

async fn refresh_media(p: &mut Project) {
    let mut fresh: Vec<(PathBuf, MediaInfo)> = Vec::new();
    for item in &p.pool {
        if let Ok(m) = crate::media::probe(&item.path).await {
            if m.kind() != item.media.kind() { fresh.push((item.path.clone(), m)); }
        }
    }
    if fresh.is_empty() { return; }
    for (path, m) in fresh {
        for i in p.pool.iter_mut().filter(|i| i.path == path) { i.media = m.clone(); }
        for c in p.clips.iter_mut().filter(|c| c.source == path) { c.media = m.clone(); }
        for o in p.overlays.iter_mut().filter(|o| o.source == path) { o.media = m.clone(); }
        for t in p.audio_tracks.iter_mut() { for c in t.clips.iter_mut().filter(|c| c.source == path) { c.media = m.clone(); } }
    }
    timeline::relayout(p);
}

#[tauri::command]
pub fn project_save(state: S, path: Option<PathBuf>) -> Result<PathBuf> {
    let path = path
        .or_else(|| state.project_path.lock().unwrap().clone())
        .ok_or_else(|| Error::InvalidEdit("no save path".into()))?;
    crate::project::persist::save(&state.project.lock().unwrap(), &path)?;
    *state.project_path.lock().unwrap() = Some(path.clone());
    Ok(path)
}

#[tauri::command]
pub fn project_set_aspect(state: S, aspect: AspectPreset) -> Project {
    state.project.lock().unwrap().aspect = aspect;
    snapshot(&state)
}

/// The captions track switch: see `Project::captions_enabled`.
#[tauri::command]
pub fn project_set_captions_enabled(state: S, enabled: bool) -> Project {
    state.project.lock().unwrap().captions_enabled = enabled;
    snapshot(&state)
}

#[tauri::command]
pub fn project_set_video_muted(state: S, muted: bool) -> Project {
    state.project.lock().unwrap().video_muted = muted;
    snapshot(&state)
}

/// V1 track fader (0–1), applied on top of every clip's own volume.
#[tauri::command]
pub fn project_set_video_volume(state: S, volume: f32) -> Project {
    state.project.lock().unwrap().video_volume = if volume.is_finite() { volume.clamp(0.0, 1.0) } else { 1.0 };
    snapshot(&state)
}

#[tauri::command]
pub fn project_set_crop(state: S, crop: Crop) -> Project {
    state.project.lock().unwrap().crop = Crop { scale: crop.scale.clamp(1.0, 4.0), x: crop.x.clamp(0.0, 1.0), y: crop.y.clamp(0.0, 1.0) };
    snapshot(&state)
}

/// Same file on disk, even when spelled differently (symlinks, `/var` vs `/private/var`, case).
/// Falls back to plain path equality when either path cannot be resolved.
fn same_file(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

/// Probe a file, reusing the pool's cached `MediaInfo` when we already know it.
async fn media_for(state: &S<'_>, path: &PathBuf) -> Result<MediaInfo> {
    let known = state.project.lock().unwrap().pool.iter().find(|i| same_file(&i.path, path)).map(|i| i.media.clone());
    match known {
        Some(m) => Ok(m),
        None => crate::media::probe(path).await,
    }
}

fn pool_insert(p: &mut Project, path: &PathBuf, media: &MediaInfo) {
    if !p.pool.iter().any(|i| same_file(&i.path, path)) {
        p.pool.push(PoolItem { id: Uuid::new_v4(), path: path.clone(), media: media.clone() });
    }
}

/// Add a file to the media pool. Video is also appended to V1 so the one-clip workflow stays one step.
#[tauri::command]
pub async fn media_import(state: S<'_>, path: PathBuf) -> Result<Project> {
    let media = media_for(&state, &path).await?;
    with_project(&state, |p| {
        pool_insert(p, &path, &media);
        if media.kind() != MediaKind::Audio {
            timeline::append(p, Clip::new(path, media));
        }
        Ok(())
    })?;
    Ok(snapshot(&state))
}

/// Add a file to the pool only (drag-and-drop, per-tab Import buttons).
#[tauri::command]
pub async fn pool_add(state: S<'_>, path: PathBuf) -> Result<Project> {
    let media = media_for(&state, &path).await?;
    with_project(&state, |p| { pool_insert(p, &path, &media); Ok(()) })?;
    Ok(snapshot(&state))
}

#[tauri::command]
pub fn pool_remove(state: S, id: Uuid) -> Project {
    state.project.lock().unwrap().pool.retain(|i| i.id != id);
    snapshot(&state)
}

/// Place a video or image from the pool (or any path) on V1 at an index.
#[tauri::command]
pub async fn clip_insert(state: S<'_>, path: PathBuf, at_index: usize) -> Result<Project> {
    let media = media_for(&state, &path).await?;
    if media.kind() == MediaKind::Audio {
        return Err(Error::Media("audio cannot go on the main track".into()));
    }
    with_project(&state, |p| {
        pool_insert(p, &path, &media);
        let c = Clip::new(path, media);
        let id = c.id;
        timeline::append(p, c);
        timeline::move_to(p, id, at_index)
    })?;
    Ok(snapshot(&state))
}

// ---------- Overlay track ----------

#[tauri::command]
pub async fn overlay_add(state: S<'_>, path: PathBuf, at: Ms, layer: u32) -> Result<Project> {
    let media = media_for(&state, &path).await?;
    if media.kind() == MediaKind::Audio {
        return Err(Error::Media("audio cannot go on the overlay track".into()));
    }
    with_project(&state, |p| {
        pool_insert(p, &path, &media);
        timeline::overlay_add(p, OverlayClip::new(path, media), at, layer);
        Ok(())
    })?;
    Ok(snapshot(&state))
}

#[tauri::command]
pub fn overlay_move(state: S, id: Uuid, at: Ms, layer: u32) -> Result<Project> {
    with_project(&state, |p| timeline::overlay_move(p, id, at, layer))?;
    Ok(snapshot(&state))
}

#[tauri::command]
pub fn overlay_layer_add(state: S) -> Project {
    timeline::overlay_layer_add(&mut state.project.lock().unwrap());
    snapshot(&state)
}

/// Mute / 0–1 fader for one overlay row (V2 = 0).
#[tauri::command]
pub fn overlay_layer_set_audio(state: S, layer: u32, muted: bool, volume: f32) -> Result<Project> {
    with_project(&state, |p| timeline::overlay_layer_set_audio(p, layer, muted, volume))?;
    Ok(snapshot(&state))
}

/// Constant opacity (0–1) for one overlay clip.
#[tauri::command]
pub fn overlay_set_opacity(state: S, id: Uuid, opacity: f32) -> Result<Project> {
    with_project(&state, |p| timeline::overlay_set_opacity(p, id, opacity))?;
    Ok(snapshot(&state))
}

/// Volume (0–2) and mute for one overlay clip.
#[tauri::command]
pub fn overlay_set_audio(state: S, id: Uuid, volume: f32, muted: bool) -> Result<Project> {
    with_project(&state, |p| timeline::overlay_set_audio(p, id, volume, muted))?;
    Ok(snapshot(&state))
}

#[tauri::command]
pub fn overlay_layer_remove(state: S, layer: u32) -> Result<Project> {
    with_project(&state, |p| timeline::overlay_layer_remove(p, layer))?;
    Ok(snapshot(&state))
}

#[tauri::command]
pub fn overlay_trim(state: S, id: Uuid, source_start: Ms, source_end: Ms) -> Result<Project> {
    with_project(&state, |p| timeline::overlay_trim(p, id, source_start, source_end))?;
    Ok(snapshot(&state))
}

#[tauri::command]
pub fn overlay_split(state: S, id: Uuid, at: Ms) -> Result<SplitResult> {
    let new_id = with_project(&state, |p| timeline::overlay_split(p, id, at))?;
    Ok(SplitResult { project: snapshot(&state), new_id })
}

#[tauri::command]
pub fn overlay_delete(state: S, id: Uuid) -> Result<Project> {
    with_project(&state, |p| timeline::overlay_delete(p, id))?;
    Ok(snapshot(&state))
}

#[tauri::command]
pub fn overlay_set_fades(state: S, id: Uuid, fade_in: Ms, fade_out: Ms) -> Result<Project> {
    with_project(&state, |p| timeline::overlay_set_fades(p, id, fade_in, fade_out))?;
    Ok(snapshot(&state))
}

#[tauri::command]
pub fn overlay_set_placement(state: S, id: Uuid, placement: Placement) -> Result<Project> {
    with_project(&state, |p| timeline::overlay_set_placement(p, id, placement))?;
    Ok(snapshot(&state))
}

// ---------- Text track (T1) ----------

#[tauri::command]
pub fn text_add(state: S, text: String, at: Ms) -> Project {
    timeline::text_add(&mut state.project.lock().unwrap(), TextClip::new(text), at);
    snapshot(&state)
}

#[tauri::command]
pub fn text_move(state: S, id: Uuid, at: Ms) -> Result<Project> {
    with_project(&state, |p| timeline::text_move(p, id, at))?;
    Ok(snapshot(&state))
}

#[tauri::command]
pub fn text_trim(state: S, id: Uuid, duration: Ms) -> Result<Project> {
    with_project(&state, |p| timeline::text_trim(p, id, duration))?;
    Ok(snapshot(&state))
}

#[tauri::command]
pub fn text_split(state: S, id: Uuid, at: Ms) -> Result<SplitResult> {
    let new_id = with_project(&state, |p| timeline::text_split(p, id, at))?;
    Ok(SplitResult { project: snapshot(&state), new_id })
}

#[tauri::command]
pub fn text_delete(state: S, id: Uuid) -> Result<Project> {
    with_project(&state, |p| timeline::text_delete(p, id))?;
    Ok(snapshot(&state))
}

#[tauri::command]
pub fn text_set_fades(state: S, id: Uuid, fade_in: Ms, fade_out: Ms) -> Result<Project> {
    with_project(&state, |p| timeline::text_set_fades(p, id, fade_in, fade_out))?;
    Ok(snapshot(&state))
}

#[tauri::command]
pub fn text_set_position(state: S, id: Uuid, x: f32, y: f32) -> Result<Project> {
    with_project(&state, |p| timeline::text_set_position(p, id, x, y))?;
    Ok(snapshot(&state))
}

#[tauri::command]
pub fn text_set_style(state: S, id: Uuid, style: TextStyle) -> Result<Project> {
    with_project(&state, |p| timeline::text_set_style(p, id, style))?;
    Ok(snapshot(&state))
}

/// Font families for the text track's picker; runs `fc-list`, so off the main thread.
#[tauri::command]
pub async fn system_fonts() -> Vec<String> {
    tokio::task::spawn_blocking(crate::render::fonts::system_fonts).await.unwrap_or_default()
}

// ---------- Audio tracks ----------

#[tauri::command]
pub fn audio_track_add(state: S, label: String) -> Project {
    timeline::audio_track_add(&mut state.project.lock().unwrap(), &label);
    snapshot(&state)
}

#[tauri::command]
pub fn audio_track_update(state: S, id: Uuid, label: String, muted: bool, volume: f32) -> Result<Project> {
    with_project(&state, |p| timeline::audio_track_update(p, id, &label, muted, volume))?;
    Ok(snapshot(&state))
}

#[tauri::command]
pub fn audio_track_remove(state: S, id: Uuid) -> Result<Project> {
    with_project(&state, |p| timeline::audio_track_remove(p, id))?;
    Ok(snapshot(&state))
}

#[tauri::command]
pub async fn audio_clip_add(state: S<'_>, track_id: Uuid, path: PathBuf, at: Ms) -> Result<Project> {
    let media = media_for(&state, &path).await?;
    if !media.has_audio {
        return Err(Error::Media("file has no audio stream".into()));
    }
    with_project(&state, |p| {
        pool_insert(p, &path, &media);
        timeline::audio_clip_add(p, track_id, AudioClip::new(path, media), at).map(|_| ())
    })?;
    Ok(snapshot(&state))
}

#[tauri::command]
pub fn audio_clip_move(state: S, id: Uuid, track_id: Uuid, at: Ms) -> Result<Project> {
    with_project(&state, |p| timeline::audio_clip_move(p, id, track_id, at))?;
    Ok(snapshot(&state))
}

#[tauri::command]
pub fn audio_clip_trim(state: S, id: Uuid, source_start: Ms, source_end: Ms) -> Result<Project> {
    with_project(&state, |p| timeline::audio_clip_trim(p, id, source_start, source_end))?;
    Ok(snapshot(&state))
}

#[tauri::command]
pub fn audio_clip_split(state: S, id: Uuid, at: Ms) -> Result<SplitResult> {
    let new_id = with_project(&state, |p| timeline::audio_clip_split(p, id, at))?;
    Ok(SplitResult { project: snapshot(&state), new_id })
}

#[tauri::command]
pub fn audio_clip_delete(state: S, id: Uuid) -> Result<Project> {
    with_project(&state, |p| timeline::audio_clip_delete(p, id))?;
    Ok(snapshot(&state))
}

#[tauri::command]
pub fn audio_clip_set(state: S, id: Uuid, volume: f32, fade_in: Ms, fade_out: Ms, muted: bool) -> Result<Project> {
    with_project(&state, |p| timeline::audio_clip_set(p, id, volume, fade_in, fade_out, muted))?;
    Ok(snapshot(&state))
}

#[tauri::command]
pub fn clip_trim(state: S, id: Uuid, source_start: Ms, source_end: Ms) -> Result<Project> {
    with_project(&state, |p| timeline::trim(p, id, source_start, source_end))?;
    Ok(snapshot(&state))
}

#[derive(Serialize)]
pub struct SplitResult {
    pub project: Project,
    pub new_id: Uuid,
}

/// Merge neighbouring pieces of one file (V1, an overlay layer or an audio track); `new_id` is the kept clip.
#[tauri::command]
pub fn clip_merge(state: S, ids: Vec<Uuid>) -> Result<SplitResult> {
    let new_id = with_project(&state, |p| timeline::merge(p, &ids))?;
    Ok(SplitResult { project: snapshot(&state), new_id })
}

#[tauri::command]
pub fn clip_split(state: S, id: Uuid, at: Ms) -> Result<SplitResult> {
    let new_id = with_project(&state, |p| timeline::split(p, id, at))?;
    Ok(SplitResult { project: snapshot(&state), new_id })
}

#[tauri::command]
pub fn clip_delete(state: S, id: Uuid) -> Result<Project> {
    with_project(&state, |p| timeline::delete(p, id))?;
    Ok(snapshot(&state))
}

#[tauri::command]
pub fn clip_move(state: S, id: Uuid, to_index: usize) -> Result<Project> {
    with_project(&state, |p| timeline::move_to(p, id, to_index))?;
    Ok(snapshot(&state))
}

#[tauri::command]
pub fn clip_set_fades(state: S, id: Uuid, fade_in: Ms, fade_out: Ms) -> Result<Project> {
    with_project(&state, |p| timeline::set_fades(p, id, fade_in, fade_out))?;
    Ok(snapshot(&state))
}

#[tauri::command]
pub fn clip_set_transition(state: S, id: Uuid, transition: Transition) -> Result<Project> {
    with_project(&state, |p| timeline::set_transition(p, id, transition))?;
    Ok(snapshot(&state))
}

/// Right-click → "Rename…" on a clip of any track. A blank name restores the file name.
#[tauri::command]
pub fn clip_rename(state: S, id: Uuid, name: String) -> Result<Project> {
    with_project(&state, |p| timeline::rename(p, id, &name))?;
    Ok(snapshot(&state))
}

/// Right-click → "Split audio from video". The V1 clip is muted and its audio becomes an audio-track clip.
#[tauri::command]
pub fn clip_detach_audio(state: S, id: Uuid, track_id: Option<Uuid>) -> Result<Project> {
    with_project(&state, |p| timeline::detach_audio(p, id, track_id).map(|_| ()))?;
    Ok(snapshot(&state))
}

#[tauri::command]
pub fn clip_set_volume(state: S, id: Uuid, volume: f32, muted: bool) -> Result<Project> {
    with_project(&state, |p| timeline::set_volume(p, id, volume, muted))?;
    Ok(snapshot(&state))
}

#[derive(Serialize)]
pub struct ThumbnailSet {
    pub path: PathBuf,
    pub interval_ms: u64,
    pub files: Vec<PathBuf>,
}

/// Filmstrip for any source path (timeline clip or pool item); cached on disk by path + mtime.
#[tauri::command]
pub async fn cache_thumbnails(path: PathBuf, duration_ms: u64) -> Result<ThumbnailSet> {
    let t = crate::cache::thumbs::generate(&path, duration_ms).await?;
    Ok(ThumbnailSet { path, interval_ms: t.interval_ms, files: t.files })
}

#[derive(Serialize)]
pub struct Waveform {
    pub bucket_ms: u32,
    pub peaks: Vec<u8>,
}

#[tauri::command]
pub async fn cache_waveform(path: PathBuf) -> Result<Waveform> {
    let peaks = crate::cache::waveform::generate(&path).await?;
    Ok(Waveform { bucket_ms: crate::cache::waveform::BUCKET_MS, peaks })
}

#[tauri::command]
pub fn export_plan(state: S, settings: ExportSettings) -> Result<ExportPlan> {
    crate::render::plan(&state.project.lock().unwrap(), &settings)
}

/// One title rasterised by the webview at output size: base64 PNG bytes, no `data:` prefix.
#[derive(Debug, Clone, Deserialize)]
pub struct TextRaster {
    pub id: Uuid,
    pub png: String,
}

/// Write the rasters to a temp dir for this export; every title inside the V1 span must have one.
fn write_text_rasters(p: &Project, texts: &[TextRaster], job_id: Uuid) -> Result<(Option<PathBuf>, Vec<crate::render::graph::TextRaster>)> {
    use base64::Engine;
    let v1_len = p.duration_ms();
    let needed: Vec<Uuid> = p.texts.iter().filter(|t| t.timeline_start < v1_len).map(|t| t.id).collect();
    if needed.is_empty() {
        return Ok((None, Vec::new()));
    }
    if let Some(missing) = needed.iter().find(|id| !texts.iter().any(|r| r.id == **id)) {
        return Err(Error::InvalidEdit(format!("text clip {missing} was not rendered")));
    }
    let dir = std::env::temp_dir().join(format!("forgevideo-{job_id}"));
    std::fs::create_dir_all(&dir)?;
    let mut out = Vec::new();
    for r in texts.iter().filter(|r| needed.contains(&r.id)) {
        let bytes = base64::engine::general_purpose::STANDARD.decode(r.png.trim())
            .map_err(|e| Error::InvalidEdit(format!("bad text raster: {e}")))?;
        let path = dir.join(format!("{}.png", r.id));
        std::fs::write(&path, bytes)?;
        out.push((r.id, path));
    }
    Ok((Some(dir), out))
}

#[tauri::command]
pub async fn export_start(app: AppHandle, state: S<'_>, settings: ExportSettings, texts: Vec<TextRaster>) -> Result<Uuid> {
    let (job_id, cancel) = state.jobs.register();
    let planned = with_project(&state, |p| {
        let (dir, rasters) = write_text_rasters(p, &texts, job_id)?;
        let plan = crate::render::plan_with_texts(p, &settings, &rasters);
        if plan.is_err() { if let Some(d) = &dir { let _ = std::fs::remove_dir_all(d); } }
        Ok((plan?, export_cues(p), dir))
    });
    let (plan, cues, raster_dir) = match planned {
        Ok(v) => v,
        Err(e) => { state.jobs.finish(job_id); return Err(e); }
    };
    let kind = "export".to_string();
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        let k = kind.clone();
        let res = crate::render::ffmpeg::run_with_progress(
            &plan.args,
            plan.duration_ms,
            move |f| {
                let _ = app2.emit(EVT_PROGRESS, JobProgress { job_id, kind: k.clone(), progress: f, message: None });
            },
            Some(cancel),
        )
        .await;
        let st: State<AppState> = app.state();
        st.jobs.finish(job_id);
        if let Some(d) = raster_dir { let _ = std::fs::remove_dir_all(d); }
        match res {
            Ok(()) => {
                let captions = write_srt(&plan.destination, &cues);
                let _ = app.emit(EVT_DONE, JobDone { job_id, kind, result: serde_json::json!({ "destination": plan.destination, "strategy": plan.strategy, "captions": captions }) });
            }
            Err(e) => {
                let _ = std::fs::remove_file(&plan.destination);
                let _ = app.emit(EVT_ERROR, JobError { job_id, kind, error: e.to_string() });
            }
        }
    });
    Ok(job_id)
}

#[tauri::command]
pub fn job_cancel(state: S, job_id: Uuid) -> bool {
    state.jobs.cancel(job_id)
}

#[derive(Serialize)]
pub struct FfmpegStatus {
    pub ffmpeg: Option<PathBuf>,
    pub ffprobe: Option<PathBuf>,
}

#[tauri::command]
pub fn ffmpeg_status() -> FfmpegStatus {
    FfmpegStatus { ffmpeg: crate::render::ffmpeg::ffmpeg_bin().ok(), ffprobe: crate::render::ffmpeg::ffprobe_bin().ok() }
}

// ---------- AI mode ----------

/// Captions travel beside the export as `<name>.srt`. `None` when there are none or it cannot be written.
/// What the `.srt` gets: the timeline cues, or nothing while the captions track is switched off.
fn export_cues(p: &Project) -> Vec<crate::project::Cue> {
    if p.captions_enabled { ai::captions::timeline_cues(p) } else { Vec::new() }
}

fn write_srt(destination: &Path, cues: &[crate::project::Cue]) -> Option<PathBuf> {
    if cues.is_empty() {
        return None;
    }
    let file = destination.with_extension("srt");
    std::fs::write(&file, ai::captions::to_srt(cues)).ok().map(|_| file)
}

/// Async so the `claude auth status` subprocess never blocks the main thread.
#[tauri::command]
pub async fn ai_status() -> ai::AiStatus {
    tokio::task::spawn_blocking(ai::status).await.expect("status never panics")
}

/// Opens Terminal on `claude auth login`; the panel polls `ai_status` until an account appears.
#[tauri::command]
pub async fn ai_claude_login() -> Result<()> {
    tokio::task::spawn_blocking(ai::claude_login).await.expect("login never panics")
}

/// Where the bundled install script lives: the app's resource dir (`target/debug` in dev).
pub fn install_script_path(app: &AppHandle) -> Result<PathBuf> {
    app.path().resource_dir().map(|d| d.join("install-ai.sh")).map_err(|e| Error::Tool(format!("no resource dir: {e}")))
}

/// Opens Terminal on `install-ai.sh`; the panel polls `ai_status` until the tools appear.
#[tauri::command]
pub async fn ai_install(app: AppHandle) -> Result<()> {
    let script = install_script_path(&app)?;
    tokio::task::spawn_blocking(move || ai::install_tools(&script)).await.expect("install never panics")
}

#[tauri::command]
pub async fn ai_claude_logout() -> Result<()> {
    tokio::task::spawn_blocking(ai::claude_logout).await.expect("logout never panics")
}

/// V1 sources with speech that have no transcript yet, in timeline order, each once.
fn untranscribed(p: &Project) -> Vec<(PathBuf, Ms)> {
    let mut out: Vec<(PathBuf, Ms)> = Vec::new();
    for c in &p.clips {
        let known = p.transcripts.iter().any(|t| t.source == c.source) || out.iter().any(|(s, _)| *s == c.source);
        if c.media.has_audio && !c.media.is_still && !known {
            out.push((c.source.clone(), c.media.duration_ms));
        }
    }
    out
}

/// Finish an AI job: apply `res` to the project it started on and tell the UI.
fn finish_ai_job(app: &AppHandle, job_id: Uuid, kind: String, project_id: Uuid, res: Result<Box<dyn FnOnce(&mut Project) + Send>>) {
    let st: State<AppState> = app.state();
    st.jobs.finish(job_id);
    let res = res.and_then(|apply| {
        with_project(&st, |p| {
            if p.id != project_id {
                return Err(Error::InvalidEdit("the project was closed while this was running".into()));
            }
            apply(p);
            timeline::relayout(p);
            Ok(p.clone())
        })
    });
    match res {
        Ok(project) => {
            let _ = app.emit(EVT_DONE, JobDone { job_id, kind, result: serde_json::json!({ "project": project }) });
        }
        Err(e) => {
            let _ = app.emit(EVT_ERROR, JobError { job_id, kind, error: e.to_string() });
        }
    }
}

/// Transcribe every V1 source that has speech. Emits `job://*` with kind `transcribe`.
#[tauri::command]
pub async fn ai_transcribe(app: AppHandle, state: S<'_>) -> Result<Uuid> {
    let (project_id, sources) = with_project(&state, |p| Ok((p.id, untranscribed(p))))?;
    if sources.is_empty() {
        return Err(Error::InvalidEdit("nothing on the main track needs transcribing".into()));
    }
    let (whisper, model) = (ai::whisper_bin()?, ai::whisper_model()?);
    let (job_id, cancel) = state.jobs.register();
    let kind = "transcribe".to_string();
    tauri::async_runtime::spawn(async move {
        let cancel = Cancel::from_oneshot(cancel);
        let n = sources.len() as f32;
        let mut done: Vec<Transcript> = Vec::new();
        let mut failed = None;
        for (i, (source, duration_ms)) in sources.into_iter().enumerate() {
            let name = source.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
            let (app2, k) = (app.clone(), kind.clone());
            let progress = move |f: f32| {
                let _ = app2.emit(EVT_PROGRESS, JobProgress { job_id, kind: k.clone(), progress: (i as f32 + f) / n, message: Some(name.clone()) });
            };
            match ai::transcribe::transcribe(&source, duration_ms, &whisper, &model, progress, cancel.clone()).await {
                Ok(cues) => done.push(Transcript { source, cues }),
                Err(e) => {
                    failed = Some(e);
                    break;
                }
            }
        }
        let res: Result<Box<dyn FnOnce(&mut Project) + Send>> = match failed {
            Some(e) => Err(e),
            None => Ok(Box::new(move |p: &mut Project| {
                p.transcripts.retain(|t| done.iter().all(|d| d.source != t.source));
                p.transcripts.extend(done);
            })),
        };
        finish_ai_job(&app, job_id, kind, project_id, res);
    });
    Ok(job_id)
}

/// Ask Claude Code for sections worth cutting into shorts. Emits `job://*` with kind `highlights`.
#[tauri::command]
pub async fn ai_find_highlights(app: AppHandle, state: S<'_>) -> Result<Uuid> {
    let (project_id, total, prompt) = with_project(&state, |p| Ok((p.id, p.duration_ms(), ai::highlights::build_prompt(p)?)))?;
    let claude = ai::claude_bin()?;
    let (job_id, cancel) = state.jobs.register();
    let kind = "highlights".to_string();
    tauri::async_runtime::spawn(async move {
        let res = ai::highlights::find(&claude, prompt, total, Cancel::from_oneshot(cancel)).await;
        let res = res.map(|found| Box::new(move |p: &mut Project| p.highlights = found) as Box<dyn FnOnce(&mut Project) + Send>);
        finish_ai_job(&app, job_id, kind, project_id, res);
    });
    Ok(job_id)
}

/// Correct a misheard caption. A blank text removes the caption.
#[tauri::command]
pub fn cue_set_text(state: S, id: Uuid, text: String) -> Result<Project> {
    with_project(&state, |p| {
        let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
        let t = p.transcripts.iter_mut().find(|t| t.cues.iter().any(|c| c.id == id)).ok_or(Error::ClipNotFound(id))?;
        if text.is_empty() {
            t.cues.retain(|c| c.id != id);
        } else if let Some(c) = t.cues.iter_mut().find(|c| c.id == id) {
            c.text = text;
        }
        Ok(())
    })?;
    Ok(snapshot(&state))
}

#[tauri::command]
pub fn highlight_delete(state: S, id: Uuid) -> Result<Project> {
    with_project(&state, |p| {
        let n = p.highlights.len();
        p.highlights.retain(|h| h.id != id);
        if p.highlights.len() == n { Err(Error::ClipNotFound(id)) } else { Ok(()) }
    })?;
    Ok(snapshot(&state))
}

/// Build the short for a highlight as its own project file beside this one. The open project is not changed.
#[tauri::command]
pub fn highlight_apply(state: S, id: Uuid) -> Result<PathBuf> {
    let project_file = state.project_path.lock().unwrap().clone()
        .ok_or_else(|| Error::InvalidEdit("save this project first, shorts are created next to it".into()))?;
    let short = with_project(&state, |p| {
        let h = p.highlights.iter().find(|h| h.id == id).ok_or(Error::ClipNotFound(id))?;
        ai::short::build_short(p, h)
    })?;
    let file = ai::short::free_file(&project_file, &short.name);
    crate::project::persist::save(&short, &file)?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    //! Commands are plain functions under the `#[tauri::command]` macro, so we drive them with a
    //! mock app's managed state. `export_start` needs an event loop and is covered by tests/export_e2e.rs
    //! at the render layer instead.
    use super::*;
    use tauri::Manager;

    fn app() -> tauri::App<tauri::test::MockRuntime> {
        let app = tauri::test::mock_app();
        app.manage(AppState::default());
        app
    }
    fn fx(n: &str) -> PathBuf {
        crate::test_util::fixture(n)
    }

    #[test]
    fn new_get_aspect_and_crop() {
        let app = app();
        let st = app.state::<AppState>();
        assert_eq!(project_get(st.clone()).name, "Untitled");
        let p = project_new(st.clone(), "Reel".into());
        assert_eq!(p.name, "Reel");
        assert!(p.clips.is_empty());
        assert_eq!(project_set_aspect(st.clone(), AspectPreset::Shorts9x16).aspect, AspectPreset::Shorts9x16);
        let p = project_set_crop(st.clone(), Crop { scale: 9.0, x: -1.0, y: 2.0 });
        assert_eq!(p.crop, Crop { scale: 4.0, x: 0.0, y: 1.0 }, "crop is clamped");
        let p = project_set_crop(st.clone(), Crop { scale: 0.2, x: 0.3, y: 0.7 });
        assert_eq!(p.crop, Crop { scale: 1.0, x: 0.3, y: 0.7 });
        assert!(project_set_video_muted(st.clone(), true).video_muted);
        assert!(project_get(st.clone()).video_muted, "video mute persisted");
        assert!(!project_set_video_muted(st.clone(), false).video_muted);
        assert_eq!(project_set_video_volume(st.clone(), 0.4).video_volume, 0.4);
        assert_eq!(project_set_video_volume(st.clone(), 7.0).video_volume, 1.0, "fader clamps to 1");
        assert_eq!(project_set_video_volume(st.clone(), f32::NAN).video_volume, 1.0);
        assert_eq!(project_set_video_volume(st.clone(), 1.0).video_volume, 1.0);
        assert_eq!(project_get(st).crop.scale, 1.0, "state persisted");
    }

    #[tokio::test]
    async fn import_then_edit_through_the_command_layer() {
        let app = app();
        let st = app.state::<AppState>();
        let p = media_import(st.clone(), fx("clip_a_720p.mp4")).await.unwrap();
        let p = media_import(st.clone(), fx("clip_b_1080p.mp4")).await.unwrap_or(p);
        assert_eq!(p.clips.len(), 2);
        let (a, b) = (p.clips[0].id, p.clips[1].id);
        let b_start = p.clips[1].timeline_start;
        assert!(b_start > 0);

        let p = clip_trim(st.clone(), a, 1000, 3000).unwrap();
        assert_eq!(p.clips[0].duration_ms(), 2000);
        assert_eq!(p.clips[1].timeline_start, 2000);
        assert!(clip_trim(st.clone(), a, 0, 50).is_err());

        let r = clip_split(st.clone(), a, 1000).unwrap();
        assert_eq!(r.project.clips.len(), 3);
        assert_eq!(r.project.clips[1].id, r.new_id);
        assert!(clip_split(st.clone(), Uuid::new_v4(), 1000).is_err());
        let m = clip_merge(st.clone(), vec![r.new_id, a]).unwrap();
        assert_eq!((m.new_id, m.project.clips.len()), (a, 2));
        assert!(clip_merge(st.clone(), vec![a]).is_err());
        clip_split(st.clone(), a, 1000).unwrap(); // back to three clips for the rest of the test

        let p = clip_set_fades(st.clone(), a, 200, 300).unwrap();
        assert_eq!((p.clips[0].fade_in, p.clips[0].fade_out), (200, 300));
        let p = clip_set_transition(st.clone(), a, Transition::CrossDissolve { ms: 400 }).unwrap();
        assert_eq!(p.clips[0].transition_out, Transition::CrossDissolve { ms: 400 });
        let p = clip_set_volume(st.clone(), a, 5.0, true).unwrap();
        assert_eq!((p.clips[0].volume, p.clips[0].muted), (2.0, true));

        let p = clip_move(st.clone(), b, 0).unwrap();
        assert_eq!(p.clips[0].id, b);
        assert_eq!(p.clips[0].timeline_start, 0);
        let p = clip_delete(st.clone(), b).unwrap();
        assert_eq!(p.clips.len(), 2);
        assert!(clip_delete(st.clone(), b).is_err());
        assert_eq!(project_get(st).clips.len(), 2);
    }

    /// What ⌘T does end to end on the Rust side: the clip under the playhead becomes two
    /// contiguous halves with the same source, and the project length does not change.
    #[tokio::test]
    async fn clip_split_command_produces_two_contiguous_halves() {
        let app = app();
        let st = app.state::<AppState>();
        let p = media_import(st.clone(), fx("clip_a_720p.mp4")).await.unwrap();
        let p = media_import(st.clone(), fx("clip_b_1080p.mp4")).await.unwrap_or(p);
        let (a, b) = (p.clips[0].id, p.clips[1].id);
        let a_len = p.clips[0].duration_ms();
        let total = p.duration_ms();
        let at = 2000;
        assert!(at < a_len, "fixture is longer than the split point");

        let r = clip_split(st.clone(), a, at).unwrap();
        let clips = &r.project.clips;
        assert_eq!(clips.len(), 3);
        assert_eq!(clips[0].id, a, "left half keeps the id");
        assert_eq!(clips[1].id, r.new_id, "right half is the new id");
        assert_eq!(clips[2].id, b);
        assert_eq!((clips[0].source_start, clips[0].source_end), (0, at));
        assert_eq!((clips[1].source_start, clips[1].source_end), (at, a_len));
        assert_eq!(clips[0].source, clips[1].source);
        assert_eq!((clips[0].timeline_start, clips[1].timeline_start, clips[2].timeline_start), (0, at, a_len));
        assert_eq!(r.project.duration_ms(), total, "splitting never changes the length");
        assert_eq!(project_get(st.clone()).clips.len(), 3, "state persisted");

        // boundaries and near-boundaries are rejected, and leave the project untouched
        assert!(matches!(clip_split(st.clone(), a, 0), Err(Error::InvalidEdit(_))));
        assert!(matches!(clip_split(st.clone(), a, at), Err(Error::InvalidEdit(_))), "exactly on the cut");
        assert!(matches!(clip_split(st.clone(), a, 50), Err(Error::InvalidEdit(_))), "left piece too short");
        assert!(matches!(clip_split(st.clone(), Uuid::new_v4(), 1000), Err(Error::ClipNotFound(_))));
        assert_eq!(project_get(st.clone()).clips.len(), 3);

        // the same for overlays and audio clips
        let p = overlay_add(st.clone(), fx("logo.png"), 1000, 0).await.unwrap();
        let o = p.overlays[0].id;
        let r = overlay_split(st.clone(), o, 3000).unwrap();
        assert_eq!(r.project.overlays.len(), 2);
        assert_eq!((r.project.overlays[0].timeline_start, r.project.overlays[0].duration_ms()), (1000, 2000));
        assert_eq!((r.project.overlays[1].id, r.project.overlays[1].timeline_start, r.project.overlays[1].duration_ms()), (r.new_id, 3000, 3000));
        let p = audio_track_add(st.clone(), "Music".into());
        let t = p.audio_tracks[0].id;
        let p = audio_clip_add(st.clone(), t, fx("music.m4a"), 500).await.unwrap();
        let c = p.audio_tracks[0].clips[0].id;
        let len = p.audio_tracks[0].clips[0].duration_ms();
        let r = audio_clip_split(st.clone(), c, 1500).unwrap();
        let cs = &r.project.audio_tracks[0].clips;
        assert_eq!(cs.len(), 2);
        assert_eq!((cs[0].source_start, cs[0].source_end, cs[0].timeline_start), (0, 1000, 500));
        assert_eq!((cs[1].id, cs[1].source_start, cs[1].source_end, cs[1].timeline_start), (r.new_id, 1000, len, 1500));
    }

    #[tokio::test]
    async fn import_puts_audio_in_the_pool_only_and_rejects_missing_files() {
        let app = app();
        let st = app.state::<AppState>();
        let p = media_import(st.clone(), fx("music.m4a")).await.unwrap();
        assert!(p.clips.is_empty(), "audio never lands on V1");
        assert_eq!(p.pool.len(), 1);
        assert_eq!(p.pool[0].media.kind(), MediaKind::Audio);
        assert!(media_import(st.clone(), PathBuf::from("/no/file.mp4")).await.is_err());
        let p = media_import(st.clone(), fx("music.m4a")).await.unwrap();
        assert_eq!(p.pool.len(), 1, "no duplicates by path");
        let p = pool_add(st.clone(), fx("logo.png")).await.unwrap();
        assert_eq!(p.pool.len(), 2);
        assert_eq!(p.pool[1].media.kind(), MediaKind::Image);
        let p = media_import(st.clone(), fx("logo.png")).await.unwrap();
        assert_eq!(p.clips.len(), 1, "imported images land on V1 like video");
        clip_delete(st.clone(), p.clips[0].id).unwrap();
        let png = p.pool[1].id;
        let p = pool_remove(st.clone(), png);
        assert_eq!(p.pool.len(), 1);
        assert!(project_get(st).clips.is_empty());
    }

    #[tokio::test]
    async fn opening_a_project_reprobes_stale_pool_media() {
        let app = app();
        let st = app.state::<AppState>();
        let dir = tempfile::tempdir().unwrap();
        // A project saved when the probe mistook the m4a for a 600x600 video.
        let mut p = Project::new("stale");
        let mut m = crate::media::probe(&fx("music.m4a")).await.unwrap();
        assert_eq!(m.kind(), MediaKind::Audio);
        m.width = 600; m.height = 600;
        assert_eq!(m.kind(), MediaKind::Video);
        p.pool.push(PoolItem { id: Uuid::new_v4(), path: fx("music.m4a"), media: m.clone() });
        timeline::append(&mut p, Clip::new(fx("music.m4a"), m));
        p.pool.push(PoolItem { id: Uuid::new_v4(), path: PathBuf::from("/gone/missing.mp4"), media: p.pool[0].media.clone() });
        let file = dir.path().join("stale.forgevideo");
        crate::project::persist::save(&p, &file).unwrap();
        let p = project_open(st.clone(), file).await.unwrap();
        assert_eq!(p.pool[0].media.kind(), MediaKind::Audio, "re-probed on open");
        assert_eq!(p.clips[0].media.kind(), MediaKind::Audio, "clips using the file are updated too");
        assert_eq!(p.pool[1].media.kind(), MediaKind::Video, "unreadable files keep what was saved");
    }

    #[tokio::test]
    async fn detach_audio_moves_the_sound_to_a_track_and_mutes_the_clip() {
        let app = app();
        let st = app.state::<AppState>();
        let p = media_import(st.clone(), fx("clip_a_720p.mp4")).await.unwrap();
        let a = p.clips[0].id;
        clip_trim(st.clone(), a, 500, 3000).unwrap();
        clip_set_fades(st.clone(), a, 200, 300).unwrap();
        let p = clip_detach_audio(st.clone(), a, None).unwrap();
        assert!(p.clips[0].muted);
        assert_eq!(p.audio_tracks.len(), 1, "a track is created when there is none");
        let ac = &p.audio_tracks[0].clips[0];
        assert_eq!((ac.source_start, ac.source_end, ac.timeline_start, ac.fade_in, ac.fade_out), (500, 3000, 0, 200, 300));
        assert_eq!(ac.source, fx("clip_a_720p.mp4"));
        // a still has nothing to detach
        let p = media_import(st.clone(), fx("logo.png")).await.unwrap();
        let png = p.clips[1].id;
        assert!(clip_detach_audio(st.clone(), png, None).is_err());
        // an explicit track is honoured
        let p = audio_track_add(st.clone(), "SFX".into());
        let sfx = p.audio_tracks[1].id;
        let p = clip_detach_audio(st.clone(), a, Some(sfx)).unwrap();
        assert_eq!(p.audio_tracks[1].clips.len(), 1);
    }

    #[tokio::test]
    async fn pool_add_ignores_symlinked_duplicates() {
        let app = app();
        let st = app.state::<AppState>();
        let dir = tempfile::tempdir().unwrap();
        let link = dir.path().join("bed.m4a");
        std::os::unix::fs::symlink(fx("music.m4a"), &link).unwrap();
        let p = pool_add(st.clone(), fx("music.m4a")).await.unwrap();
        assert_eq!(p.pool.len(), 1);
        let p = pool_add(st.clone(), link.clone()).await.unwrap();
        assert_eq!(p.pool.len(), 1, "a symlink to a pooled file is the same file");
        let p = media_import(st.clone(), link).await.unwrap();
        assert_eq!(p.pool.len(), 1);
        assert!(same_file(Path::new("/definitely/missing/a"), Path::new("/definitely/missing/a")));
        assert!(!same_file(Path::new("/definitely/missing/a"), Path::new("/definitely/missing/b")));
    }

    #[test]
    fn text_track_commands_and_raster_staging() {
        let app = app();
        let st = app.state::<AppState>();
        let p = text_add(st.clone(), "Hello".into(), 1000);
        assert_eq!(p.texts.len(), 1);
        let id = p.texts[0].id;
        assert_eq!((p.texts[0].timeline_start, p.texts[0].duration, p.texts[0].style.text.as_str()), (1000, 5000, "Hello"));
        assert_eq!(text_trim(st.clone(), id, 2000).unwrap().texts[0].duration, 2000);
        assert_eq!(text_move(st.clone(), id, 500).unwrap().texts[0].timeline_start, 500);
        let p = text_set_fades(st.clone(), id, 100, 200).unwrap();
        assert_eq!((p.texts[0].fade_in, p.texts[0].fade_out), (100, 200));
        let p = text_set_position(st.clone(), id, 0.2, 0.3).unwrap();
        assert_eq!((p.texts[0].x, p.texts[0].y), (0.2, 0.3));
        let style = TextStyle { text: "Big".into(), font: "Impact".into(), size: 0.2, color: "#ff0000".into(), backdrop: Some(crate::project::Backdrop { color: "#000000".into(), opacity: 0.5 }) };
        let p = text_set_style(st.clone(), id, style.clone()).unwrap();
        assert_eq!(p.texts[0].style, style);
        let r = text_split(st.clone(), id, 1500).unwrap();
        assert_eq!(r.project.texts.len(), 2);
        assert_eq!(text_delete(st.clone(), r.new_id).unwrap().texts.len(), 1);
        assert!(text_delete(st.clone(), r.new_id).is_err());

        // raster staging: a title inside V1 needs a PNG; titles past the end do not
        let mut p = project_get(st.clone());
        let mi = MediaInfo { duration_ms: 3000, width: 1280, height: 720, fps: crate::project::Rational { num: 30, den: 1 }, codec: "h264".into(), container: "mov".into(), has_audio: true, audio_codec: Some("aac".into()), sample_rate: Some(48000), rotation: 0, is_still: false };
        let c = Clip::new(fx("clip_a_720p.mp4"), mi);
        timeline::append(&mut p, c);
        let job = Uuid::new_v4();
        assert!(write_text_rasters(&p, &[], job).is_err(), "missing raster is refused");
        let png = std::fs::read(fx("logo.png")).unwrap();
        let b64 = { use base64::Engine; base64::engine::general_purpose::STANDARD.encode(&png) };
        assert!(write_text_rasters(&p, &[TextRaster { id, png: "not base64!".into() }], job).is_err());
        let (dir, rasters) = write_text_rasters(&p, &[TextRaster { id, png: b64 }], job).unwrap();
        assert_eq!(rasters.len(), 1);
        assert_eq!(std::fs::read(&rasters[0].1).unwrap(), png);
        std::fs::remove_dir_all(dir.unwrap()).unwrap();
        timeline::text_move(&mut p, id, 10_000).unwrap();
        assert_eq!(write_text_rasters(&p, &[], job).unwrap(), (None, Vec::new()), "no titles in V1: nothing staged");
    }

    #[tokio::test]
    async fn overlay_and_audio_track_commands() {
        let app = app();
        let st = app.state::<AppState>();
        media_import(st.clone(), fx("clip_a_720p.mp4")).await.unwrap();
        // overlay: png badge, then a video overlay
        assert!(overlay_add(st.clone(), fx("music.m4a"), 0, 0).await.is_err(), "audio is not an overlay");
        let p = overlay_add(st.clone(), fx("logo.png"), 1000, 0).await.unwrap();
        assert_eq!(p.overlays.len(), 1);
        assert!(p.overlays[0].media.is_still);
        assert_eq!(p.overlays[0].placement, Placement::BADGE);
        assert_eq!(p.pool.len(), 2, "placed media is also in the pool");
        let o = p.overlays[0].id;
        let p = overlay_trim(st.clone(), o, 0, 2000).unwrap();
        assert_eq!(p.overlays[0].duration_ms(), 2000);
        let p = overlay_move(st.clone(), o, 500, 0).unwrap();
        assert_eq!(p.overlays[0].timeline_start, 500);
        let p = overlay_set_fades(st.clone(), o, 100, 100).unwrap();
        assert_eq!((p.overlays[0].fade_in, p.overlays[0].fade_out), (100, 100));
        let p = overlay_set_placement(st.clone(), o, Placement { scale: 0.5, x: 0.1, y: 0.1 }).unwrap();
        assert_eq!(p.overlays[0].placement.scale, 0.5);
        let r = overlay_split(st.clone(), o, 1500).unwrap();
        assert_eq!(r.project.overlays.len(), 2);
        let p = overlay_delete(st.clone(), r.new_id).unwrap();
        assert_eq!(p.overlays.len(), 1);
        let p = overlay_add(st.clone(), fx("clip_b_1080p.mp4"), 3000, 1).await.unwrap();
        assert_eq!(p.overlays.len(), 2);
        assert_eq!(p.overlay_layers, 2, "grew to fit layer 1");
        let p = overlay_layer_add(st.clone());
        assert_eq!(p.overlay_layers, 3);
        let p = overlay_layer_remove(st.clone(), 1).unwrap();
        assert_eq!((p.overlay_layers, p.overlays.len()), (2, 1));
        // audio tracks
        let p = audio_track_add(st.clone(), "Music".into());
        let t = p.audio_tracks[0].id;
        assert!(audio_clip_add(st.clone(), t, fx("logo.png"), 0).await.is_err(), "no audio stream");
        let p = audio_clip_add(st.clone(), t, fx("music.m4a"), 2000).await.unwrap();
        let c = p.audio_tracks[0].clips[0].id;
        assert_eq!(p.audio_tracks[0].clips[0].timeline_start, 2000);
        let p = clip_rename(st.clone(), c, "Theme".into()).unwrap();
        assert_eq!(p.audio_tracks[0].clips[0].name.as_deref(), Some("Theme"));
        let p = clip_rename(st.clone(), o, "Logo".into()).unwrap();
        assert_eq!(p.overlays[0].name.as_deref(), Some("Logo"));
        let p = clip_rename(st.clone(), p.clips[0].id, "".into()).unwrap();
        assert_eq!(p.clips[0].name, None);
        assert!(clip_rename(st.clone(), Uuid::new_v4(), "x".into()).is_err());
        let ov = overlay_add(st.clone(), fx("clip_b_1080p.mp4"), 0, 0).await.unwrap();
        let oid = ov.overlays[0].id;
        let p = overlay_set_opacity(st.clone(), oid, 0.25).unwrap();
        assert_eq!(p.overlays[0].opacity, 0.25);
        let p = overlay_set_audio(st.clone(), oid, 0.6, true).unwrap();
        assert_eq!((p.overlays[0].volume, p.overlays[0].muted), (0.6, true));
        let p = overlay_layer_set_audio(st.clone(), 0, true, 0.3).unwrap();
        assert_eq!((p.overlay_audio[0].muted, p.overlay_audio[0].volume), (true, 0.3));
        assert!(overlay_layer_set_audio(st.clone(), 9, false, 1.0).is_err());
        let p = audio_track_update(st.clone(), t, "Bed".into(), true, 0.5).unwrap();
        assert_eq!((p.audio_tracks[0].label.as_str(), p.audio_tracks[0].muted, p.audio_tracks[0].volume), ("Bed", true, 0.5));
        let p = audio_track_add(st.clone(), "SFX".into());
        let sfx = p.audio_tracks[1].id;
        let p = audio_clip_move(st.clone(), c, sfx, 0).unwrap();
        assert_eq!(p.audio_tracks[1].clips.len(), 1);
        let p = audio_clip_trim(st.clone(), c, 500, 1500).unwrap();
        assert_eq!(p.audio_tracks[1].clips[0].duration_ms(), 1000);
        let p = audio_clip_set(st.clone(), c, 0.4, 100, 100, false).unwrap();
        assert_eq!(p.audio_tracks[1].clips[0].volume, 0.4);
        let r = audio_clip_split(st.clone(), c, 500).unwrap();
        assert_eq!(r.project.audio_tracks[1].clips.len(), 2);
        let p = audio_clip_delete(st.clone(), r.new_id).unwrap();
        assert_eq!(p.audio_tracks[1].clips.len(), 1);
        let p = audio_track_remove(st.clone(), t).unwrap();
        assert_eq!(p.audio_tracks.len(), 1);
        // the export plan now needs a re-encode
        let s = ExportSettings { destination: PathBuf::from("/tmp/x.mp4"), quality: crate::render::Quality::Standard, audio_only: false };
        let plan = export_plan(st.clone(), s).unwrap();
        assert_eq!(plan.strategy, crate::render::Strategy::HardwareEncode);
        assert!(plan.reasons.contains(&"overlay track".to_string()));
        // clip_insert puts video or a still at an index on V1; audio is refused
        let p = clip_insert(st.clone(), fx("clip_b_1080p.mp4"), 0).await.unwrap();
        assert_eq!(p.clips.len(), 2);
        assert!(p.clips[0].source.ends_with("clip_b_1080p.mp4"));
        let p = clip_insert(st.clone(), fx("logo.png"), 1).await.unwrap();
        assert!(p.clips[1].media.is_still);
        assert_eq!(p.clips[1].duration_ms(), 5000);
        assert!(clip_insert(st, fx("music.m4a"), 0).await.is_err());
    }

    #[tokio::test]
    async fn save_open_round_trip_and_remembered_path() {
        let app = app();
        let st = app.state::<AppState>();
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(project_save(st.clone(), None), Err(Error::InvalidEdit(_))), "no path yet");
        project_new(st.clone(), "Saved".into());
        media_import(st.clone(), fx("clip_a_720p.mp4")).await.unwrap();
        let file = dir.path().join("s.forgevideo");
        assert_eq!(project_save(st.clone(), Some(file.clone())).unwrap(), file);
        assert!(file.is_file());
        // subsequent save without a path reuses it
        assert_eq!(project_save(st.clone(), None).unwrap(), file);

        project_new(st.clone(), "Other".into());
        assert!(matches!(project_save(st.clone(), None), Err(Error::InvalidEdit(_))), "new project forgets the path");
        let p = project_open(st.clone(), file.clone()).await.unwrap();
        assert_eq!(p.name, "Saved");
        assert_eq!(p.clips.len(), 1);
        assert_eq!(project_save(st.clone(), None).unwrap(), file, "open remembers the path");
        assert!(project_open(st, dir.path().join("missing.forgevideo")).await.is_err());
    }

    #[tokio::test]
    async fn export_plan_and_cache_commands() {
        let app = app();
        let st = app.state::<AppState>();
        let s = ExportSettings { destination: PathBuf::from("/tmp/x.mp4"), quality: crate::render::Quality::Standard, audio_only: false };
        assert!(export_plan(st.clone(), s.clone()).is_err(), "empty timeline");
        let p = media_import(st.clone(), fx("clip_a_720p.mp4")).await.unwrap();
        let id = p.clips[0].id;
        let plan = export_plan(st.clone(), s.clone()).unwrap();
        assert_eq!(plan.strategy, crate::render::Strategy::StreamCopy, "untouched 16:9 clip copies packets");
        assert_eq!(plan.output, (1280, 720), "stream copy keeps the source size");
        assert!(plan.reasons.is_empty());
        let plan = export_plan(st.clone(), ExportSettings { audio_only: true, ..s.clone() }).unwrap();
        assert_eq!(plan.strategy, crate::render::Strategy::AudioOnly);

        let src = p.clips[0].source.clone();
        let t = cache_thumbnails(src.clone(), p.clips[0].media.duration_ms).await.unwrap();
        assert_eq!(t.path, src);
        assert!(t.files.len() > 10);
        assert!(t.files.iter().all(|f| f.extension().unwrap() == "jpg"));
        assert!(cache_thumbnails(PathBuf::from("/nope.mp4"), 1000).await.is_err());
        let _ = id;

        let w = cache_waveform(fx("clip_a_720p.mp4")).await.unwrap();
        assert_eq!(w.bucket_ms, 10);
        assert!(w.peaks.len() > 400);
        assert!(cache_waveform(PathBuf::from("/nope.mp4")).await.is_err());
    }

    #[test]
    fn job_cancel_and_ffmpeg_status() {
        let app = app();
        let st = app.state::<AppState>();
        assert!(!job_cancel(st.clone(), Uuid::new_v4()));
        let (id, _rx) = st.jobs.register();
        assert!(job_cancel(st.clone(), id));
        assert!(!job_cancel(st, id));
        let s = ffmpeg_status();
        assert!(s.ffmpeg.is_some() && s.ffprobe.is_some());
        let j = serde_json::to_value(&s).unwrap();
        assert!(j["ffmpeg"].is_string());
    }

    fn with_captions(st: &State<AppState>) -> (Uuid, Uuid) {
        use crate::project::{Cue, Highlight, Range};
        let mut p = st.project.lock().unwrap();
        let source = p.clips[0].source.clone();
        let cues = vec![
            Cue { id: Uuid::new_v4(), start: 0, end: 1500, text: "So here is the thing".into() },
            Cue { id: Uuid::new_v4(), start: 1500, end: 3000, text: "nobody tells you".into() },
        ];
        let cue = cues[0].id;
        p.transcripts.push(Transcript { source, cues });
        let h = Highlight {
            id: Uuid::new_v4(), title: "The thing".into(), reason: "hook".into(), start: 500, end: 3000,
            keep: vec![Range { start: 500, end: 1500 }, Range { start: 2000, end: 3000 }], fade_in: 0, fade_out: 200, notes: vec![],
        };
        let id = h.id;
        p.highlights.push(h);
        (cue, id)
    }

    #[tokio::test]
    async fn captions_can_be_corrected_and_highlights_dismissed() {
        let app = app();
        let st = app.state::<AppState>();
        media_import(st.clone(), fx("clip_a_720p.mp4")).await.unwrap();
        let (cue, h) = with_captions(&st);
        let p = cue_set_text(st.clone(), cue, "  So here's   the thing \n".into()).unwrap();
        assert_eq!(p.transcripts[0].cues[0].text, "So here's the thing");
        let p = cue_set_text(st.clone(), cue, "  ".into()).unwrap();
        assert_eq!(p.transcripts[0].cues.len(), 1, "blank removes the caption");
        assert!(matches!(cue_set_text(st.clone(), cue, "x".into()), Err(Error::ClipNotFound(_))));
        let p = highlight_delete(st.clone(), h).unwrap();
        assert!(p.highlights.is_empty());
        assert!(matches!(highlight_delete(st.clone(), h), Err(Error::ClipNotFound(_))));
        assert!(untranscribed(&project_get(st.clone())).is_empty(), "already has a transcript");
        let p = media_import(st.clone(), fx("clip_b_1080p.mp4")).await.unwrap();
        media_import(st.clone(), fx("clip_b_1080p.mp4")).await.unwrap();
        media_import(st.clone(), fx("logo.png")).await.unwrap();
        assert_eq!(untranscribed(&project_get(st)), vec![(p.clips[1].source.clone(), p.clips[1].media.duration_ms)], "once per source, no stills");
    }

    #[tokio::test]
    async fn applying_a_highlight_writes_a_short_beside_a_saved_project() {
        let app = app();
        let st = app.state::<AppState>();
        let dir = tempfile::tempdir().unwrap();
        project_new(st.clone(), "Episode".into());
        media_import(st.clone(), fx("clip_a_720p.mp4")).await.unwrap();
        let (_, h) = with_captions(&st);
        assert!(matches!(highlight_apply(st.clone(), h), Err(Error::InvalidEdit(m)) if m.contains("save this project first")));
        let file = dir.path().join("Episode.forgevideo");
        project_save(st.clone(), Some(file.clone())).unwrap();
        let before = project_get(st.clone());

        let short = highlight_apply(st.clone(), h).unwrap();
        assert_eq!(short, dir.path().join("Episode - The thing.forgevideo"));
        assert_eq!(project_get(st.clone()), before, "the open project is not changed");
        assert_eq!(project_save(st.clone(), None).unwrap(), file, "and still saves to its own file");
        let s = crate::project::persist::load(&short).unwrap();
        assert_eq!(s.aspect, AspectPreset::Shorts9x16);
        assert_eq!(s.clips.iter().map(|c| (c.source_start, c.source_end)).collect::<Vec<_>>(), vec![(500, 1500), (2000, 3000)]);
        assert_eq!(s.clips[1].fade_out, 200);
        assert_eq!(ai::captions::timeline_cues(&s).len(), 2);
        assert!(s.highlights.is_empty());
        assert_eq!(highlight_apply(st.clone(), h).unwrap(), dir.path().join("Episode - The thing 2.forgevideo"));
        assert!(matches!(highlight_apply(st, Uuid::new_v4()), Err(Error::ClipNotFound(_))));
    }

    #[tokio::test]
    async fn srt_is_written_beside_the_export_only_when_there_are_captions() {
        let app = app();
        let st = app.state::<AppState>();
        media_import(st.clone(), fx("clip_a_720p.mp4")).await.unwrap();
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("out.mp4");
        assert_eq!(write_srt(&dest, &[]), None);
        with_captions(&st);
        let cues = export_cues(&project_get(st.clone()));
        assert_eq!(cues.len(), ai::captions::timeline_cues(&project_get(st.clone())).len());
        assert!(!project_set_captions_enabled(st.clone(), false).captions_enabled);
        assert!(export_cues(&project_get(st.clone())).is_empty(), "captions off: nothing reaches the .srt");
        assert!(project_set_captions_enabled(st.clone(), true).captions_enabled);
        assert_eq!(write_srt(&dest, &cues), Some(dir.path().join("out.srt")));
        assert!(std::fs::read_to_string(dir.path().join("out.srt")).unwrap().starts_with("1\n00:00:00,000 --> 00:00:01,500\nSo here is the thing\n"));
        assert_eq!(write_srt(&dir.path().join("missing/out.mp4"), &cues), None);
        let j = serde_json::to_value(ai_status().await).unwrap();
        assert!(j["model_path"].is_string());
    }

    #[test]
    fn split_result_serializes_new_id_for_the_store() {
        let app = app();
        let st = app.state::<AppState>();
        let r = SplitResult { project: project_get(st), new_id: Uuid::nil() };
        let j = serde_json::to_value(&r).unwrap();
        assert_eq!(j["new_id"], Uuid::nil().to_string());
        assert!(j["project"]["clips"].is_array());
    }
}
