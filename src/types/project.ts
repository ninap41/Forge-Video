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

/** `scale` 1 fills the frame; up to CROP_SCALE_MAX zooms in, down to CROP_SCALE_MIN zooms out onto black. */
export interface Crop { scale: number; x: number; y: number }
export const CROP_SCALE_MIN = 0.25;
export const CROP_SCALE_MAX = 4;
/** The Inspector's zoom slider: -100 … 0 … 100 ↔ CROP_SCALE_MIN … 1 … CROP_SCALE_MAX. */
export const zoomToScale = (z: number) => (z >= 0 ? 1 + (CROP_SCALE_MAX - 1) * (z / 100) : 1 - (1 - CROP_SCALE_MIN) * (-z / 100));
export const scaleToZoom = (s: number) => (s >= 1 ? ((s - 1) / (CROP_SCALE_MAX - 1)) * 100 : -((1 - s) / (1 - CROP_SCALE_MIN)) * 100);

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
  /** 0–2 like V1 clips; files saved before overlays carried sound load muted. */
  volume: number;
  muted: boolean;
  /** 0–1 constant opacity for composite shots, multiplied with the fades. */
  opacity: number;
}
/** Mute and 0–1 fader for one overlay row (`Project.overlay_audio[layer]`). */
export interface LayerAudio { muted: boolean; volume: number }
export const LAYER_AUDIO_DEFAULT: LayerAudio = { muted: false, volume: 1 };

/** Look of a title; mirrors `TextStyle` in model.rs. Rendered by the canvas rasteriser in `src/utils/textRaster.ts`. */
export interface TextStyle {
  /** "\n" starts a new line; ≤ 500 chars. */
  text: string;
  /** CSS family name the system knows. */
  font: string;
  /** Line height as a fraction of the frame height, 0.02–0.4. */
  size: number;
  /** #rrggbb */
  color: string;
  /** Rounded-rectangle behind the text; null = none. */
  backdrop: Backdrop | null;
}
export interface Backdrop { color: string; opacity: number }
export const TEXT_DEFAULT_FONT = "Quicksand";
export const TEXT_STYLE_DEFAULT: TextStyle = { text: "Title", font: TEXT_DEFAULT_FONT, size: 0.08, color: "#ffffff", backdrop: null };
export const TEXT_SIZE_MIN = 0.02;
export const TEXT_SIZE_MAX = 0.4;

/** A title on a text row (`layer` 0 = T1): centred at x/y (normalised), burned in above every overlay. */
export interface TextClip {
  id: string;
  name?: string | null;
  layer: number;
  style: TextStyle;
  timeline_start: Ms;
  duration: Ms;
  fade_in: Ms;
  fade_out: Ms;
  x: number;
  y: number;
}
/** A rasterised title handed to `export_start`: base64 PNG bytes without the `data:` prefix. */
export interface TextRaster { id: string; png: string }

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
  /** Track fader 0–1. */
  volume: number;
  clips: AudioClip[];
}
export const TRACK_LABEL_PRESETS = ["Music", "SFX", "Narration", "Other"] as const;

export interface PoolItem { id: string; path: string; media: MediaInfo }

/** One caption line, in *source* time. `timelineCues` maps them through the V1 clips. */
export interface Cue { id: string; start: Ms; end: Ms; text: string }
export interface Transcript { source: string; cues: Cue[] }
export interface Range { start: Ms; end: Ms }
/** A pinned span of the timeline (timeline time; clamped, never shifted, by edits). */
export interface Loop { id: string; name: string; start: Ms; end: Ms }
export const sameRange = (a: Range | null | undefined, b: Range | null | undefined) => !!a && !!b && a.start === b.start && a.end === b.end;
/** A section worth cutting into a short, with the plan to build it. Timeline time. */
export interface Highlight {
  id: string;
  title: string;
  reason: string;
  start: Ms;
  end: Ms;
  /** The parts of start..end that make the cut, in order. */
  keep: Range[];
  fade_in: Ms;
  fade_out: Ms;
  /** Suggestions the app cannot apply by itself. */
  notes: string[];
}

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
  /** One entry per overlay row; Rust keeps it the same length as `overlay_layers`. */
  overlay_audio: LayerAudio[];
  /** Titles on the text rows T1, T2, … */
  texts: TextClip[];
  /** Number of text rows shown (≥ 1). */
  text_layers: number;
  /** Track-level mute for V1. */
  video_muted: boolean;
  /** V1 fader 0–1, multiplied into each clip's volume. */
  video_volume: number;
  audio_tracks: AudioTrack[];
  pool: PoolItem[];
  transcripts: Transcript[];
  /** Off = caption row dimmed, no preview caption, no .srt on export. Old files load as on. */
  captions_enabled: boolean;
  highlights: Highlight[];
  loops: Loop[];
  fps: Rational | null;
}

export type Quality = "Draft" | "Standard" | "High";
export type Strategy = "StreamCopy" | "HardwareEncode" | "AudioOnly";

/** `range` renders only that span of the timeline (a pinned loop or the current selection). */
export interface ExportSettings { destination: string; quality: Quality; audio_only: boolean; range: Range | null }

export interface ExportPlan {
  strategy: Strategy;
  duration_ms: Ms;
  output: [number, number];
  destination: string;
  reasons: string[];
  args: string[];
}

export interface JobProgress { job_id: string; kind: string; progress: number; message: string | null }
export interface ExportResult { destination: string; strategy: Strategy; captions?: string | null }
/** Export jobs finish with a file; AI jobs (`transcribe`, `highlights`) with the updated project. */
export interface JobDone { job_id: string; kind: string; result: ExportResult | { project: Project } }
export interface JobError { job_id: string; kind: string; error: string }

/** `account` is the signed-in Claude email (null until Sign in completes). */
export interface AiStatus { whisper: string | null; model: string | null; model_path: string; claude: string | null; account: string | null }

/** Works for V1, overlay and audio clips alike. */
export const clipDuration = (c: { source_start: Ms; source_end: Ms }): Ms => c.source_end - c.source_start;
export const clipEnd = (c: { timeline_start: Ms; source_start: Ms; source_end: Ms }): Ms => c.timeline_start + clipDuration(c);
export const projectDuration = (p: Project): Ms => {
  const last = p.clips[p.clips.length - 1];
  return last ? last.timeline_start + clipDuration(last) : 0;
};
/** What a clip is called on the timeline: its own name, else the file name (a title: its first line). */
export const clipName = (c: { name?: string | null; source?: string; style?: TextStyle }): string =>
  c.name || (c.style ? (c.style.text.split("\n").find((l) => l.trim()) ?? "Title") : (c.source?.split("/").pop() ?? c.source ?? ""));
/** Timeline end of a title. */
export const textEnd = (t: TextClip): Ms => t.timeline_start + t.duration;
export const transitionMs = (t: Transition): Ms => (t.type === "None" ? 0 : t.ms);
