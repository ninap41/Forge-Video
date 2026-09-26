//! Thin `#[tauri::command]` layer. Every mutating command returns the full `Project` so the
//! frontend store can replace its state wholesale.

use crate::error::{Error, Result};
use crate::jobs::{JobDone, JobError, JobProgress, EVT_DONE, EVT_ERROR, EVT_PROGRESS};
use crate::project::{AspectPreset, AudioTrack, Clip, Crop, Ms, Project, Transition};
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
pub fn project_set_crop(state: S, crop: Crop) -> Project {
    state.project.lock().unwrap().crop = Crop { scale: crop.scale.clamp(1.0, 4.0), x: crop.x.clamp(0.0, 1.0), y: crop.y.clamp(0.0, 1.0) };
    snapshot(&state)
}

#[tauri::command]
pub async fn media_import(state: S<'_>, path: PathBuf) -> Result<Project> {
    let media = crate::media::probe(&path).await?;
    if media.width == 0 {
        return Err(Error::Media("file has no video stream; use it as music instead".into()));
    }
    with_project(&state, |p| {
        timeline::append(p, Clip::new(path, media));
        Ok(())
    })?;
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

#[tauri::command]
pub async fn music_set(state: S<'_>, path: Option<PathBuf>) -> Result<Project> {
    let track = match path {
        Some(p) => {
            let m = crate::media::probe(&p).await?;
            if !m.has_audio {
                return Err(Error::Media("file has no audio stream".into()));
            }
            Some(AudioTrack::new(p, m.duration_ms))
        }
        None => None,
    };
    state.project.lock().unwrap().music = track;
    Ok(snapshot(&state))
}

#[tauri::command]
pub fn music_update(state: S, track: AudioTrack) -> Result<Project> {
    let mut p = state.project.lock().unwrap();
    match &mut p.music {
        Some(m) => {
            let mut t = track;
            t.source = m.source.clone();
            t.duration_ms = m.duration_ms;
            t.trim_end = t.trim_end.min(t.duration_ms);
            t.trim_start = t.trim_start.min(t.trim_end);
            t.volume = t.volume.clamp(0.0, 2.0);
            *m = t;
        }
        None => return Err(Error::InvalidEdit("no music track".into())),
    }
    drop(p);
    Ok(snapshot(&state))
}

#[derive(Serialize)]
pub struct ThumbnailSet {
    pub clip_id: Uuid,
    pub interval_ms: u64,
    pub files: Vec<PathBuf>,
}

fn clip_source(state: &S, id: Uuid) -> Result<(PathBuf, u64)> {
    let p = state.project.lock().unwrap();
    let c = p.clips.iter().find(|c| c.id == id).ok_or(Error::ClipNotFound(id))?;
    Ok((c.source.clone(), c.media.duration_ms))
}

#[tauri::command]
pub async fn cache_thumbnails(state: S<'_>, clip_id: Uuid) -> Result<ThumbnailSet> {
    let (src, dur) = clip_source(&state, clip_id)?;
    let t = crate::cache::thumbs::generate(&src, dur).await?;
    Ok(ThumbnailSet { clip_id, interval_ms: t.interval_ms, files: t.files })
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

    #[tokio::test]
    async fn import_rejects_audio_only_and_missing_files() {
        let app = app();
        let st = app.state::<AppState>();
        let e = media_import(st.clone(), fx("music.m4a")).await.unwrap_err();
        assert!(e.to_string().contains("no video stream"), "{e}");
        assert!(media_import(st.clone(), PathBuf::from("/no/file.mp4")).await.is_err());
        assert!(project_get(st).clips.is_empty(), "failed imports leave no clip");
    }

    #[tokio::test]
    async fn music_set_and_update_validate_and_clamp() {
        let app = app();
        let st = app.state::<AppState>();
        assert!(matches!(music_update(st.clone(), AudioTrack::new(PathBuf::from("/x"), 1)), Err(Error::InvalidEdit(_))));
        let p = music_set(st.clone(), Some(fx("music.m4a"))).await.unwrap();
        let m = p.music.clone().unwrap();
        assert_eq!(m.source, fx("music.m4a"));
        assert!(m.duration_ms > 0);
        assert_eq!(m.volume, 0.5);

        let mut t = m.clone();
        t.source = PathBuf::from("/attacker.m4a");
        t.duration_ms = 1;
        t.trim_end = 999_999;
        t.trim_start = 999_999;
        t.volume = 7.0;
        t.fade_in = 100;
        t.timeline_start = 2500;
        let p = music_update(st.clone(), t).unwrap();
        let u = p.music.unwrap();
        assert_eq!(u.source, m.source, "source and duration cannot be changed by update");
        assert_eq!(u.duration_ms, m.duration_ms);
        assert_eq!(u.trim_end, m.duration_ms);
        assert_eq!(u.trim_start, u.trim_end);
        assert_eq!(u.volume, 2.0);
        assert_eq!((u.fade_in, u.timeline_start), (100, 2500));

        assert!(music_set(st.clone(), Some(PathBuf::from("/no/file.m4a"))).await.is_err());
        assert!(project_get(st.clone()).music.is_some(), "failed set keeps the old track");
        assert!(music_set(st, None).await.unwrap().music.is_none());
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

        let t = cache_thumbnails(st.clone(), id).await.unwrap();
        assert_eq!(t.clip_id, id);
        assert!(t.files.len() > 10);
        assert!(t.files.iter().all(|f| f.extension().unwrap() == "jpg"));
        assert!(cache_thumbnails(st.clone(), Uuid::new_v4()).await.is_err());

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
