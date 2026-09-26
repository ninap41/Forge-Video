import { vi } from "vitest";
import type { AudioTrack, Clip, ExportPlan, MediaInfo, Project } from "../types/project";

let n = 0;
export const media = (over: Partial<MediaInfo> = {}): MediaInfo => ({
  duration_ms: 5000, width: 1920, height: 1080, fps: { num: 30, den: 1 }, codec: "h264", container: "mov,mp4",
  has_audio: true, audio_codec: "aac", sample_rate: 48000, rotation: 0, ...over,
});

export const clip = (over: Partial<Clip> = {}): Clip => {
  n += 1;
  const m = over.media ?? media();
  return {
    id: over.id ?? `clip-${n}`, source: `/videos/clip${n}.mp4`, media: m, source_start: 0, source_end: m.duration_ms,
    timeline_start: 0, fade_in: 0, fade_out: 0, volume: 1, muted: false, transition_out: { type: "None" }, ...over,
  };
};

/** Lays clips out contiguously like timeline::relayout (ignoring transitions unless set). */
export const project = (clips: Clip[] = [], over: Partial<Project> = {}): Project => {
  let cursor = 0;
  for (const c of clips) {
    c.timeline_start = cursor;
    cursor += c.source_end - c.source_start;
    cursor -= c.transition_out.type === "None" ? 0 : c.transition_out.ms;
  }
  return { version: 1, id: "proj-1", name: "Test", aspect: "YouTube16x9", crop: { scale: 1, x: 0.5, y: 0.5 }, clips, music: null, fps: null, ...over };
};

export const music = (over: Partial<AudioTrack> = {}): AudioTrack => ({
  source: "/audio/bed.m4a", duration_ms: 20000, timeline_start: 0, trim_start: 0, trim_end: 20000, volume: 0.5, fade_in: 0, fade_out: 0, muted: false, ...over,
});

/** Two-clip project shared by the store/App tests: a = 5 s, b = 4 s. */
export const baseProject = () => project([clip({ id: "a", source: "/videos/a.mp4" }), clip({ id: "b", source: "/videos/b.mp4", source_end: 4000 })]);

export type MockApi = ReturnType<typeof mockApi>;

/** A full mock of src/api/tauri.ts. Every call resolves with `resolveProject` unless overridden. */
export function mockApi(p: Project) {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const resolved = <T,>(v: T) => vi.fn<(...args: any[]) => Promise<T>>(() => Promise.resolve(v));
  return {
    projectGet: resolved(p), projectNew: resolved(p), projectOpen: resolved(p), projectSave: resolved("/saved.forgevideo"),
    setAspect: resolved(p), setCrop: resolved(p), mediaImport: resolved(p), clipTrim: resolved(p),
    clipSplit: resolved({ project: p, new_id: "new" }), clipDelete: resolved(p), clipMove: resolved(p), clipSetFades: resolved(p),
    clipSetTransition: resolved(p), clipSetVolume: resolved(p), musicSet: resolved(p), musicUpdate: resolved(p),
    cacheThumbnails: resolved({ clip_id: "x", interval_ms: 200, files: [] as string[] }), cacheWaveform: resolved({ bucket_ms: 10, peaks: [] as number[] }),
    exportPlan: resolved<ExportPlan>({ strategy: "StreamCopy", duration_ms: 5000, output: [1920, 1080], destination: "/o.mp4", reasons: [], args: [] }),
    exportStart: resolved("job-1"), jobCancel: resolved(true), ffmpegStatus: resolved<{ ffmpeg: string | null; ffprobe: string | null }>({ ffmpeg: "/opt/homebrew/bin/ffmpeg", ffprobe: "/opt/homebrew/bin/ffprobe" }),
    onJobProgress: resolved<() => void>(() => {}), onJobDone: resolved<() => void>(() => {}), onJobError: resolved<() => void>(() => {}),
    assetUrl: (path: string) => `asset://localhost${path}`,
  };
}

/** Make every project-returning command echo `p`, so edits do not wipe the store during component tests. */
export function resolveWith(api: MockApi, p: Project) {
  for (const k of ["projectGet", "projectNew", "projectOpen", "setAspect", "setCrop", "mediaImport", "clipTrim", "clipDelete", "clipMove",
    "clipSetFades", "clipSetTransition", "clipSetVolume", "musicSet", "musicUpdate"] as const) {
    api[k].mockImplementation(() => Promise.resolve(p) as never);
  }
  api.clipSplit.mockImplementation(() => Promise.resolve({ project: p, new_id: "new" }));
}
