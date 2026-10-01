// Every Rust command in one place. The UI never builds an ffmpeg argument.
import { invoke } from "@tauri-apps/api/core";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { convertFileSrc } from "@tauri-apps/api/core";
import type {
  AiStatus, AspectPreset, Crop, ExportPlan, ExportSettings, JobDone, JobError, JobProgress, Ms, Placement, Project, TextRaster, TextStyle, Transition,
} from "../types/project";

export type SplitResult = { project: Project; new_id: string };

/** Rust takes integer milliseconds; the playhead and drags are floats. Round at the boundary, once. */
const ms = (v: Ms): Ms => Math.round(v);

export const api = {
  projectGet: () => invoke<Project>("project_get"),
  projectNew: (name: string) => invoke<Project>("project_new", { name }),
  projectOpen: (path: string) => invoke<Project>("project_open", { path }),
  projectSave: (path?: string) => invoke<string>("project_save", { path: path ?? null }),
  setAspect: (aspect: AspectPreset) => invoke<Project>("project_set_aspect", { aspect }),
  setCrop: (crop: Crop) => invoke<Project>("project_set_crop", { crop }),
  setCaptionsEnabled: (enabled: boolean) => invoke<Project>("project_set_captions_enabled", { enabled }),
  setVideoMuted: (muted: boolean) => invoke<Project>("project_set_video_muted", { muted }),
  setVideoVolume: (volume: number) => invoke<Project>("project_set_video_volume", { volume }),

  mediaImport: (path: string) => invoke<Project>("media_import", { path }),
  clipTrim: (id: string, sourceStart: Ms, sourceEnd: Ms) => invoke<Project>("clip_trim", { id, sourceStart: ms(sourceStart), sourceEnd: ms(sourceEnd) }),
  clipSplit: (id: string, at: Ms) => invoke<SplitResult>("clip_split", { id, at: ms(at) }),
  /** Joins neighbouring pieces of one file (any row) back into one clip; `new_id` is the kept clip. */
  clipMerge: (ids: string[]) => invoke<SplitResult>("clip_merge", { ids }),
  /** Video from the pool onto V1 at an index. */
  clipInsert: (path: string, atIndex: number) => invoke<Project>("clip_insert", { path, atIndex }),
  clipDelete: (id: string) => invoke<Project>("clip_delete", { id }),
  clipMove: (id: string, toIndex: number) => invoke<Project>("clip_move", { id, toIndex }),
  clipSetFades: (id: string, fadeIn: Ms, fadeOut: Ms) => invoke<Project>("clip_set_fades", { id, fadeIn: ms(fadeIn), fadeOut: ms(fadeOut) }),
  clipSetTransition: (id: string, transition: Transition) => invoke<Project>("clip_set_transition", { id, transition }),
  clipSetVolume: (id: string, volume: number, muted: boolean) => invoke<Project>("clip_set_volume", { id, volume, muted }),
  /** Rename a clip on any track (V1, overlay, audio). Blank restores the file name. */
  clipRename: (id: string, name: string) => invoke<Project>("clip_rename", { id, name }),
  /** Split audio from video: the clip's sound becomes an audio-track clip and the video clip is muted. */
  clipDetachAudio: (id: string, trackId: string | null = null) => invoke<Project>("clip_detach_audio", { id, trackId }),

  poolAdd: (path: string) => invoke<Project>("pool_add", { path }),
  poolRemove: (id: string) => invoke<Project>("pool_remove", { id }),

  overlayAdd: (path: string, at: Ms, layer: number) => invoke<Project>("overlay_add", { path, at: ms(at), layer }),
  overlayMove: (id: string, at: Ms, layer: number) => invoke<Project>("overlay_move", { id, at: ms(at), layer }),
  overlayLayerAdd: () => invoke<Project>("overlay_layer_add"),
  overlayLayerRemove: (layer: number) => invoke<Project>("overlay_layer_remove", { layer }),
  overlayTrim: (id: string, sourceStart: Ms, sourceEnd: Ms) => invoke<Project>("overlay_trim", { id, sourceStart: ms(sourceStart), sourceEnd: ms(sourceEnd) }),
  overlaySplit: (id: string, at: Ms) => invoke<SplitResult>("overlay_split", { id, at: ms(at) }),
  overlayDelete: (id: string) => invoke<Project>("overlay_delete", { id }),
  overlaySetFades: (id: string, fadeIn: Ms, fadeOut: Ms) => invoke<Project>("overlay_set_fades", { id, fadeIn: ms(fadeIn), fadeOut: ms(fadeOut) }),
  overlaySetOpacity: (id: string, opacity: number) => invoke<Project>("overlay_set_opacity", { id, opacity }),
  overlaySetAudio: (id: string, volume: number, muted: boolean) => invoke<Project>("overlay_set_audio", { id, volume, muted }),
  overlayLayerSetAudio: (layer: number, muted: boolean, volume: number) => invoke<Project>("overlay_layer_set_audio", { layer, muted, volume }),
  overlaySetPlacement: (id: string, placement: Placement) => invoke<Project>("overlay_set_placement", { id, placement }),

  textAdd: (text: string, at: Ms) => invoke<Project>("text_add", { text, at: ms(at) }),
  textMove: (id: string, at: Ms) => invoke<Project>("text_move", { id, at: ms(at) }),
  textTrim: (id: string, duration: Ms) => invoke<Project>("text_trim", { id, duration: ms(duration) }),
  textSplit: (id: string, at: Ms) => invoke<SplitResult>("text_split", { id, at: ms(at) }),
  textDelete: (id: string) => invoke<Project>("text_delete", { id }),
  textSetFades: (id: string, fadeIn: Ms, fadeOut: Ms) => invoke<Project>("text_set_fades", { id, fadeIn: ms(fadeIn), fadeOut: ms(fadeOut) }),
  textSetPosition: (id: string, x: number, y: number) => invoke<Project>("text_set_position", { id, x, y }),
  textSetStyle: (id: string, style: TextStyle) => invoke<Project>("text_set_style", { id, style }),
  /** Every installed font family (bundled ones first); falls back to a short macOS list. */
  systemFonts: () => invoke<string[]>("system_fonts"),

  audioTrackAdd: (label: string) => invoke<Project>("audio_track_add", { label }),
  audioTrackUpdate: (id: string, label: string, muted: boolean, volume: number) => invoke<Project>("audio_track_update", { id, label, muted, volume }),
  audioTrackRemove: (id: string) => invoke<Project>("audio_track_remove", { id }),
  audioClipAdd: (trackId: string, path: string, at: Ms) => invoke<Project>("audio_clip_add", { trackId, path, at: ms(at) }),
  audioClipMove: (id: string, trackId: string, at: Ms) => invoke<Project>("audio_clip_move", { id, trackId, at: ms(at) }),
  audioClipTrim: (id: string, sourceStart: Ms, sourceEnd: Ms) => invoke<Project>("audio_clip_trim", { id, sourceStart: ms(sourceStart), sourceEnd: ms(sourceEnd) }),
  audioClipSplit: (id: string, at: Ms) => invoke<SplitResult>("audio_clip_split", { id, at: ms(at) }),
  audioClipDelete: (id: string) => invoke<Project>("audio_clip_delete", { id }),
  audioClipSet: (id: string, volume: number, fadeIn: Ms, fadeOut: Ms, muted: boolean) =>
    invoke<Project>("audio_clip_set", { id, volume, fadeIn: ms(fadeIn), fadeOut: ms(fadeOut), muted }),

  cacheThumbnails: (path: string, durationMs: Ms) => invoke<{ path: string; interval_ms: number; files: string[] }>("cache_thumbnails", { path, durationMs: ms(durationMs) }),
  cacheWaveform: (path: string) => invoke<{ bucket_ms: number; peaks: number[] }>("cache_waveform", { path }),

  exportPlan: (settings: ExportSettings) => invoke<ExportPlan>("export_plan", { settings }),
  /** `texts` carries one PNG per title inside the V1 span, rasterised at the output size. */
  exportStart: (settings: ExportSettings, texts: TextRaster[] = []) => invoke<string>("export_start", { settings, texts }),
  jobCancel: (jobId: string) => invoke<boolean>("job_cancel", { jobId }),
  ffmpegStatus: () => invoke<{ ffmpeg: string | null; ffprobe: string | null }>("ffmpeg_status"),

  aiStatus: () => invoke<AiStatus>("ai_status"),
  /** Opens Terminal on `claude auth login`; poll `aiStatus` until `account` is set. */
  aiClaudeLogin: () => invoke<void>("ai_claude_login"),
  aiClaudeLogout: () => invoke<void>("ai_claude_logout"),
  /** Opens Terminal on the bundled install script; poll `aiStatus` until the tools appear. */
  aiInstall: () => invoke<void>("ai_install"),
  /** Speech to captions for every V1 source; resolves to a job id, the project arrives with `job://done`. */
  aiTranscribe: () => invoke<string>("ai_transcribe"),
  /** Claude Code picks sections for shorts; resolves to a job id. */
  aiFindHighlights: () => invoke<string>("ai_find_highlights"),
  cueSetText: (id: string, text: string) => invoke<Project>("cue_set_text", { id, text }),
  highlightDelete: (id: string) => invoke<Project>("highlight_delete", { id }),
  /** Builds the short as a new project file next to this one; resolves to its path. */
  highlightApply: (id: string) => invoke<string>("highlight_apply", { id }),

  onJobProgress: (cb: (e: JobProgress) => void): Promise<UnlistenFn> => listen<JobProgress>("job://progress", (e) => cb(e.payload)),
  onJobDone: (cb: (e: JobDone) => void): Promise<UnlistenFn> => listen<JobDone>("job://done", (e) => cb(e.payload)),
  onJobError: (cb: (e: JobError) => void): Promise<UnlistenFn> => listen<JobError>("job://error", (e) => cb(e.payload)),

  /** URL the WebView can load for a local media/thumbnail file. */
  assetUrl: (path: string) => convertFileSrc(path),
  /** Puts `text` on the system clipboard (clipboard-manager plugin; WebKit's own API is unreliable here). */
  copyText: (text: string) => writeText(text),
};
