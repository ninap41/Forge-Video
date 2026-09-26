//! Thin `#[tauri::command]` layer. Every mutating command returns the full `Project` so the
//! frontend store can replace its state wholesale.

use crate::error::{Error, Result};
use crate::jobs::{JobDone, JobError, JobProgress, EVT_DONE, EVT_ERROR, EVT_PROGRESS};
use crate::project::{AspectPreset, AudioClip, Clip, Crop, MediaInfo, MediaKind, Ms, OverlayClip, Placement, PoolItem, Project, Transition};
use crate::render::{ExportPlan, ExportSettings};
use crate::{timeline, AppState};
use serde::Serialize;
use std::path::PathBuf;
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

#[tauri::command]
pub fn project_open(state: S, path: PathBuf) -> Result<Project> {
    let p = crate::project::persist::load(&path)?;
    *state.project.lock().unwrap() = p;
    *state.project_path.lock().unwrap() = Some(path);
    Ok(snapshot(&state))
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

#[tauri::command]
pub fn project_set_video_muted(state: S, muted: bool) -> Project {
    state.project.lock().unwrap().video_muted = muted;
    snapshot(&state)
}

#[tauri::command]
pub fn project_set_crop(state: S, crop: Crop) -> Project {
    state.project.lock().unwrap().crop = Crop { scale: crop.scale.clamp(1.0, 4.0), x: crop.x.clamp(0.0, 1.0), y: crop.y.clamp(0.0, 1.0) };
    snapshot(&state)
}

/// Probe a file, reusing the pool's cached `MediaInfo` when we already know it.
async fn media_for(state: &S<'_>, path: &PathBuf) -> Result<MediaInfo> {
    let known = state.project.lock().unwrap().pool.iter().find(|i| &i.path == path).map(|i| i.media.clone());
    match known {
        Some(m) => Ok(m),
        None => crate::media::probe(path).await,
    }
}

fn pool_insert(p: &mut Project, path: &PathBuf, media: &MediaInfo) {
    if !p.pool.iter().any(|i| &i.path == path) {
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

// ---------- Audio tracks ----------

#[tauri::command]
pub fn audio_track_add(state: S, label: String) -> Project {
    timeline::audio_track_add(&mut state.project.lock().unwrap(), &label);
    snapshot(&state)
}

#[tauri::command]
pub fn audio_track_update(state: S, id: Uuid, label: String, muted: bool) -> Result<Project> {
    with_project(&state, |p| timeline::audio_track_update(p, id, &label, muted))?;
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

#[tauri::command]
pub async fn export_start(app: AppHandle, state: S<'_>, settings: ExportSettings) -> Result<Uuid> {
    let plan = crate::render::plan(&state.project.lock().unwrap(), &settings)?;
    let (job_id, cancel) = state.jobs.register();
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
        match res {
            Ok(()) => {
                let _ = app.emit(EVT_DONE, JobDone { job_id, kind, result: serde_json::json!({ "destination": plan.destination, "strategy": plan.strategy }) });
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
        let p = audio_track_update(st.clone(), t, "Bed".into(), true).unwrap();
        assert_eq!((p.audio_tracks[0].label.as_str(), p.audio_tracks[0].muted), ("Bed", true));
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
        let p = project_open(st.clone(), file.clone()).unwrap();
        assert_eq!(p.name, "Saved");
        assert_eq!(p.clips.len(), 1);
        assert_eq!(project_save(st.clone(), None).unwrap(), file, "open remembers the path");
        assert!(project_open(st, dir.path().join("missing.forgevideo")).is_err());
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
