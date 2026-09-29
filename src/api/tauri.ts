// Every Rust command in one place. The UI never builds an ffmpeg argument.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { convertFileSrc } from "@tauri-apps/api/core";
import type {
  AspectPreset, Crop, ExportPlan, ExportSettings, JobDone, JobError, JobProgress, Ms, Placement, Project, Transition,
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
  setVideoMuted: (muted: boolean) => invoke<Project>("project_set_video_muted", { muted }),

  mediaImport: (path: string) => invoke<Project>("media_import", { path }),
  clipTrim: (id: string, sourceStart: Ms, sourceEnd: Ms) => invoke<Project>("clip_trim", { id, sourceStart: ms(sourceStart), sourceEnd: ms(sourceEnd) }),
  clipSplit: (id: string, at: Ms) => invoke<SplitResult>("clip_split", { id, at: ms(at) }),
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
  overlaySetPlacement: (id: string, placement: Placement) => invoke<Project>("overlay_set_placement", { id, placement }),

  audioTrackAdd: (label: string) => invoke<Project>("audio_track_add", { label }),
  audioTrackUpdate: (id: string, label: string, muted: boolean) => invoke<Project>("audio_track_update", { id, label, muted }),
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
  exportStart: (settings: ExportSettings) => invoke<string>("export_start", { settings }),
  jobCancel: (jobId: string) => invoke<boolean>("job_cancel", { jobId }),
  ffmpegStatus: () => invoke<{ ffmpeg: string | null; ffprobe: string | null }>("ffmpeg_status"),

  onJobProgress: (cb: (e: JobProgress) => void): Promise<UnlistenFn> => listen<JobProgress>("job://progress", (e) => cb(e.payload)),
  onJobDone: (cb: (e: JobDone) => void): Promise<UnlistenFn> => listen<JobDone>("job://done", (e) => cb(e.payload)),
  onJobError: (cb: (e: JobError) => void): Promise<UnlistenFn> => listen<JobError>("job://error", (e) => cb(e.payload)),

  /** URL the WebView can load for a local media/thumbnail file. */
  assetUrl: (path: string) => convertFileSrc(path),
};
