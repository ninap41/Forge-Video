pub mod ai;
pub mod cache;
pub mod capture;
pub mod commands;
pub mod error;
pub mod jobs;
pub mod media;
pub mod project;
pub mod render;
pub mod timeline;
mod test_util;

use std::sync::Mutex;

pub struct AppState {
    pub project: Mutex<project::Project>,
    pub project_path: Mutex<Option<std::path::PathBuf>>,
    pub jobs: jobs::JobRegistry,
}

impl Default for AppState {
    fn default() -> Self {
        AppState {
            project: Mutex::new(project::Project::default()),
            project_path: Mutex::new(None),
            jobs: jobs::JobRegistry::default(),
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            commands::project_get,
            commands::project_new,
            commands::project_open,
            commands::project_save,
            commands::project_set_aspect,
            commands::project_set_crop,
            commands::project_set_video_muted,
            commands::project_set_captions_enabled,
            commands::project_set_video_volume,
            commands::media_import,
            commands::clip_trim,
            commands::clip_split,
            commands::clip_merge,
            commands::clip_delete,
            commands::clip_move,
            commands::clip_set_fades,
            commands::clip_set_transition,
            commands::clip_set_volume,
            commands::clip_detach_audio,
            commands::clip_rename,
            commands::pool_add,
            commands::pool_remove,
            commands::clip_insert,
            commands::overlay_add,
            commands::overlay_move,
            commands::overlay_layer_add,
            commands::overlay_layer_remove,
            commands::overlay_layer_set_audio,
            commands::overlay_set_audio,
            commands::overlay_set_opacity,
            commands::overlay_trim,
            commands::overlay_split,
            commands::overlay_delete,
            commands::overlay_set_fades,
            commands::overlay_set_placement,
            commands::text_add,
            commands::text_move,
            commands::text_trim,
            commands::text_split,
            commands::text_delete,
            commands::text_set_fades,
            commands::text_set_position,
            commands::text_set_style,
            commands::text_layer_add,
            commands::text_layer_remove,
            commands::loop_add,
            commands::loop_remove,
            commands::system_fonts,
            commands::audio_track_add,
            commands::audio_track_update,
            commands::audio_track_remove,
            commands::audio_clip_add,
            commands::audio_clip_move,
            commands::audio_clip_trim,
            commands::audio_clip_split,
            commands::audio_clip_delete,
            commands::audio_clip_set,
            commands::cache_thumbnails,
            commands::cache_waveform,
            commands::export_plan,
            commands::export_start,
            commands::job_cancel,
            commands::ffmpeg_status,
            commands::ai_status,
            commands::ai_claude_login,
            commands::ai_install,
            commands::ai_claude_logout,
            commands::ai_transcribe,
            commands::ai_find_highlights,
            commands::cue_set_text,
            commands::highlight_delete,
            commands::highlight_apply,
        ])
        .run(tauri::generate_context!())
        .expect("error while running ForgeVideo");
}
