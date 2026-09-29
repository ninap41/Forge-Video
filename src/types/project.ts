// Twin of src-tauri/src/project/model.rs. Keep in sync by hand (V1).

export type Ms = number;

export interface Rational { num: number; den: number }

export interface MediaInfo {
  duration_ms: Ms;
  width: number;
  height: number;
  fps: Rational;
  codec: string;
  container: string;
  has_audio: boolean;
  audio_codec: string | null;
  sample_rate: number | null;
  rotation: number;
  /** Still image (png/jpeg/webp): no intrinsic duration, no audio. */
  is_still: boolean;
}

export type MediaKind = "Video" | "Audio" | "Image";
export const mediaKind = (m: MediaInfo): MediaKind => (m.is_still ? "Image" : m.width > 0 ? "Video" : "Audio");

/** Length a still gets when first placed on the overlay track. */
export const STILL_DEFAULT_MS: Ms = 5000;

export type AspectPreset = "YouTube16x9" | "Shorts9x16" | "Square1x1" | "LinkedIn4x5";

export const ASPECT_PRESETS: { id: AspectPreset; label: string; sub: string; w: number; h: number }[] = [
  { id: "YouTube16x9", label: "YouTube", sub: "16:9", w: 1920, h: 1080 },
  { id: "Shorts9x16", label: "Shorts / Reels", sub: "9:16", w: 1080, h: 1920 },
  { id: "Square1x1", label: "Square", sub: "1:1", w: 1080, h: 1080 },
  { id: "LinkedIn4x5", label: "LinkedIn", sub: "4:5", w: 1080, h: 1350 },
];

export interface Crop { scale: number; x: number; y: number }

export type Transition =
  | { type: "None" }
  | { type: "CrossDissolve"; ms: number }
  | { type: "DipToBlack"; ms: number };

export interface Clip {
  id: string;
  source: string;
  /** User-given name; absent or null = show the file name. */
  name?: string | null;
  media: MediaInfo;
  source_start: Ms;
  source_end: Ms;
  timeline_start: Ms;
  fade_in: Ms;
  fade_out: Ms;
  volume: number;
  muted: boolean;
  transition_out: Transition;
}

/** Where an overlay sits: scale = overlay width / output width, x/y = normalized centre. */
export interface Placement { scale: number; x: number; y: number }
export const PLACEMENT_FULL: Placement = { scale: 1, x: 0.5, y: 0.5 };
export const PLACEMENT_BADGE: Placement = { scale: 0.35, x: 0.85, y: 0.85 };

/** Overlay (V2) clip: free-positioned, silent, composited above V1. */
export interface OverlayClip {
  id: string;
  source: string;
  /** User-given name; absent or null = show the file name. */
  name?: string | null;
  media: MediaInfo;
  source_start: Ms;
  source_end: Ms;
  timeline_start: Ms;
  fade_in: Ms;
  fade_out: Ms;
  placement: Placement;
  /** Overlay row: 0 = V2, 1 = V3, … Higher layers composite on top. */
  layer: number;
}

export interface AudioClip {
  id: string;
  source: string;
  /** User-given name; absent or null = show the file name. */
  name?: string | null;
  media: MediaInfo;
  source_start: Ms;
  source_end: Ms;
  timeline_start: Ms;
  volume: number;
  fade_in: Ms;
  fade_out: Ms;
  muted: boolean;
}

export interface AudioTrack {
  id: string;
  label: string;
  muted: boolean;
  clips: AudioClip[];
}
export const TRACK_LABEL_PRESETS = ["Music", "SFX", "Narration", "Other"] as const;

export interface PoolItem { id: string; path: string; media: MediaInfo }

export interface Project {
  version: number;
  id: string;
  name: string;
  aspect: AspectPreset;
  crop: Crop;
  clips: Clip[];
  overlays: OverlayClip[];
  /** Number of overlay rows shown (≥ 1). */
  overlay_layers: number;
  /** Track-level mute for V1. */
  video_muted: boolean;
  audio_tracks: AudioTrack[];
  pool: PoolItem[];
  fps: Rational | null;
}

export type Quality = "Draft" | "Standard" | "High";
export type Strategy = "StreamCopy" | "HardwareEncode" | "AudioOnly";

export interface ExportSettings { destination: string; quality: Quality; audio_only: boolean }

export interface ExportPlan {
  strategy: Strategy;
  duration_ms: Ms;
  output: [number, number];
  destination: string;
  reasons: string[];
  args: string[];
}

export interface JobProgress { job_id: string; kind: string; progress: number; message: string | null }
export interface JobDone { job_id: string; kind: string; result: { destination: string; strategy: Strategy } }
export interface JobError { job_id: string; kind: string; error: string }

/** Works for V1, overlay and audio clips alike. */
export const clipDuration = (c: { source_start: Ms; source_end: Ms }): Ms => c.source_end - c.source_start;
export const clipEnd = (c: { timeline_start: Ms; source_start: Ms; source_end: Ms }): Ms => c.timeline_start + clipDuration(c);
export const projectDuration = (p: Project): Ms => {
  const last = p.clips[p.clips.length - 1];
  return last ? last.timeline_start + clipDuration(last) : 0;
};
/** What a clip is called on the timeline: its own name, else the file name. */
export const clipName = (c: { name?: string | null; source: string }): string => c.name || (c.source.split("/").pop() ?? c.source);
export const transitionMs = (t: Transition): Ms => (t.type === "None" ? 0 : t.ms);
