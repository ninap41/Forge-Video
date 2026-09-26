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
}

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

export interface AudioTrack {
  source: string;
  duration_ms: Ms;
  timeline_start: Ms;
  trim_start: Ms;
  trim_end: Ms;
  volume: number;
  fade_in: Ms;
  fade_out: Ms;
  muted: boolean;
}

export interface Project {
  version: number;
  id: string;
  name: string;
  aspect: AspectPreset;
  crop: Crop;
  clips: Clip[];
  music: AudioTrack | null;
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

export const clipDuration = (c: Clip): Ms => c.source_end - c.source_start;
export const projectDuration = (p: Project): Ms => {
  const last = p.clips[p.clips.length - 1];
  return last ? last.timeline_start + clipDuration(last) : 0;
};
export const transitionMs = (t: Transition): Ms => (t.type === "None" ? 0 : t.ms);
