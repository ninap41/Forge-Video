// Every Rust command in one place. The UI never builds an ffmpeg argument.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { convertFileSrc } from "@tauri-apps/api/core";
import type {
  AspectPreset, AudioTrack, Crop, ExportPlan, ExportSettings, JobDone, JobError, JobProgress, Ms, Project, Transition,
} from "../types/project";

export const api = {
  projectGet: () => invoke<Project>("project_get"),
  projectNew: (name: string) => invoke<Project>("project_new", { name }),
  projectOpen: (path: string) => invoke<Project>("project_open", { path }),
  projectSave: (path?: string) => invoke<string>("project_save", { path: path ?? null }),
  setAspect: (aspect: AspectPreset) => invoke<Project>("project_set_aspect", { aspect }),
  setCrop: (crop: Crop) => invoke<Project>("project_set_crop", { crop }),

  mediaImport: (path: string) => invoke<Project>("media_import", { path }),
  clipTrim: (id: string, sourceStart: Ms, sourceEnd: Ms) => invoke<Project>("clip_trim", { id, sourceStart, sourceEnd }),
  clipSplit: (id: string, at: Ms) => invoke<{ project: Project; new_id: string }>("clip_split", { id, at }),
  clipDelete: (id: string) => invoke<Project>("clip_delete", { id }),
  clipMove: (id: string, toIndex: number) => invoke<Project>("clip_move", { id, toIndex }),
  clipSetFades: (id: string, fadeIn: Ms, fadeOut: Ms) => invoke<Project>("clip_set_fades", { id, fadeIn, fadeOut }),
  clipSetTransition: (id: string, transition: Transition) => invoke<Project>("clip_set_transition", { id, transition }),
  clipSetVolume: (id: string, volume: number, muted: boolean) => invoke<Project>("clip_set_volume", { id, volume, muted }),

  musicSet: (path: string | null) => invoke<Project>("music_set", { path }),
  musicUpdate: (track: AudioTrack) => invoke<Project>("music_update", { track }),

  cacheThumbnails: (clipId: string) => invoke<{ clip_id: string; interval_ms: number; files: string[] }>("cache_thumbnails", { clipId }),
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
