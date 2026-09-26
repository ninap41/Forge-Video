import { vi } from "vitest";
import type { AudioClip, AudioTrack, Clip, ExportPlan, MediaInfo, OverlayClip, PoolItem, Project } from "../types/project";
import { PLACEMENT_BADGE, PLACEMENT_FULL, STILL_DEFAULT_MS } from "../types/project";

let n = 0;
export const media = (over: Partial<MediaInfo> = {}): MediaInfo => ({
  duration_ms: 5000, width: 1920, height: 1080, fps: { num: 30, den: 1 }, codec: "h264", container: "mov,mp4",
  has_audio: true, audio_codec: "aac", sample_rate: 48000, rotation: 0, is_still: false, ...over,
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
  return {
    version: 2, id: "proj-1", name: "Test", aspect: "YouTube16x9", crop: { scale: 1, x: 0.5, y: 0.5 }, clips,
    overlays: [], overlay_layers: 1, video_muted: false, audio_tracks: [], pool: [], fps: null, ...over,
  };
};

export const audioMedia = (over: Partial<MediaInfo> = {}): MediaInfo => media({ width: 0, height: 0, codec: "", duration_ms: 20000, ...over });
export const stillMedia = (over: Partial<MediaInfo> = {}): MediaInfo =>
  media({ width: 400, height: 300, codec: "png", container: "png_pipe", has_audio: false, audio_codec: null, sample_rate: null, is_still: true, duration_ms: STILL_DEFAULT_MS, ...over });

export const audioClip = (over: Partial<AudioClip> = {}): AudioClip => {
  n += 1;
  const m = over.media ?? audioMedia();
  return { id: over.id ?? `aud-${n}`, source: `/audio/bed${n}.m4a`, media: m, source_start: 0, source_end: m.duration_ms, timeline_start: 0, volume: 1, fade_in: 0, fade_out: 0, muted: false, ...over };
};
export const audioTrack = (clips: AudioClip[] = [], over: Partial<AudioTrack> = {}): AudioTrack => {
  n += 1;
  return { id: over.id ?? `trk-${n}`, label: "Music", muted: false, clips, ...over };
};
export const overlay = (over: Partial<OverlayClip> = {}): OverlayClip => {
  n += 1;
  const m = over.media ?? stillMedia();
  return {
    id: over.id ?? `ov-${n}`, source: m.is_still ? `/images/logo${n}.png` : `/videos/broll${n}.mp4`, media: m, source_start: 0, source_end: m.duration_ms,
    timeline_start: 0, fade_in: 0, fade_out: 0, placement: m.is_still ? { ...PLACEMENT_BADGE } : { ...PLACEMENT_FULL }, layer: 0, ...over,
  };
};
export const poolItem = (over: Partial<PoolItem> = {}): PoolItem => {
  n += 1;
  const m = over.media ?? media();
  return { id: over.id ?? `pool-${n}`, path: m.is_still ? `/images/img${n}.png` : m.width ? `/videos/clip${n}.mp4` : `/audio/a${n}.m4a`, media: m, ...over };
};

/** Two-clip project shared by the store/App tests: a = 5 s, b = 4 s. */
export const baseProject = () => project([clip({ id: "a", source: "/videos/a.mp4" }), clip({ id: "b", source: "/videos/b.mp4", source_end: 4000 })]);

export type MockApi = ReturnType<typeof mockApi>;

/** A full mock of src/api/tauri.ts. Every call resolves with `resolveProject` unless overridden. */
export function mockApi(p: Project) {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const resolved = <T,>(v: T) => vi.fn<(...args: any[]) => Promise<T>>(() => Promise.resolve(v));
  return {
    projectGet: resolved(p), projectNew: resolved(p), projectOpen: resolved(p), projectSave: resolved("/saved.forgevideo"),
    setAspect: resolved(p), setCrop: resolved(p), setVideoMuted: resolved(p), mediaImport: resolved(p), clipTrim: resolved(p),
    clipSplit: resolved({ project: p, new_id: "new" }), clipDelete: resolved(p), clipMove: resolved(p), clipSetFades: resolved(p),
    clipSetTransition: resolved(p), clipSetVolume: resolved(p), clipInsert: resolved(p),
    poolAdd: resolved(p), poolRemove: resolved(p),
    overlayAdd: resolved(p), overlayMove: resolved(p), overlayLayerAdd: resolved(p), overlayLayerRemove: resolved(p), overlayTrim: resolved(p), overlaySplit: resolved({ project: p, new_id: "new" }),
    overlayDelete: resolved(p), overlaySetFades: resolved(p), overlaySetPlacement: resolved(p),
    audioTrackAdd: resolved(p), audioTrackUpdate: resolved(p), audioTrackRemove: resolved(p),
    audioClipAdd: resolved(p), audioClipMove: resolved(p), audioClipTrim: resolved(p), audioClipSplit: resolved({ project: p, new_id: "new" }),
    audioClipDelete: resolved(p), audioClipSet: resolved(p),
    cacheThumbnails: resolved({ path: "x", interval_ms: 200, files: [] as string[] }), cacheWaveform: resolved({ bucket_ms: 10, peaks: [] as number[] }),
    exportPlan: resolved<ExportPlan>({ strategy: "StreamCopy", duration_ms: 5000, output: [1920, 1080], destination: "/o.mp4", reasons: [], args: [] }),
    exportStart: resolved("job-1"), jobCancel: resolved(true), ffmpegStatus: resolved<{ ffmpeg: string | null; ffprobe: string | null }>({ ffmpeg: "/opt/homebrew/bin/ffmpeg", ffprobe: "/opt/homebrew/bin/ffprobe" }),
    onJobProgress: resolved<() => void>(() => {}), onJobDone: resolved<() => void>(() => {}), onJobError: resolved<() => void>(() => {}),
    assetUrl: (path: string) => `asset://localhost${path}`,
  };
}

/** Make every project-returning command echo `p`, so edits do not wipe the store during component tests. */
export function resolveWith(api: MockApi, p: Project) {
  for (const k of ["projectGet", "projectNew", "projectOpen", "setAspect", "setCrop", "setVideoMuted", "mediaImport", "clipTrim", "clipDelete", "clipMove",
    "clipSetFades", "clipSetTransition", "clipSetVolume", "clipInsert", "poolAdd", "poolRemove",
    "overlayAdd", "overlayMove", "overlayLayerAdd", "overlayLayerRemove", "overlayTrim", "overlayDelete", "overlaySetFades", "overlaySetPlacement",
    "audioTrackAdd", "audioTrackUpdate", "audioTrackRemove", "audioClipAdd", "audioClipMove", "audioClipTrim", "audioClipDelete", "audioClipSet"] as const) {
    api[k].mockImplementation(() => Promise.resolve(p) as never);
  }
  for (const k of ["clipSplit", "overlaySplit", "audioClipSplit"] as const) {
    api[k].mockImplementation(() => Promise.resolve({ project: p, new_id: "new" }));
  }
}

/** Mimics Rust `timeline::split` on V1 so tests can assert the real outcome of ⌘T, not just the call. */
export function fakeSplit(p: Project, id: string, at: number): { project: Project; new_id: string } {
  const i = p.clips.findIndex((c) => c.id === id);
  if (i < 0) throw new Error(`clip not found: ${id}`);
  const c = p.clips[i];
  const len = c.source_end - c.source_start;
  if (at <= c.timeline_start || at >= c.timeline_start + len) throw new Error("invalid edit: split point outside clip");
  const offset = at - c.timeline_start;
  if (offset < 100 || len - offset < 100) throw new Error("invalid edit: split would create a clip that is too short");
  const cut = c.source_start + offset;
  const left = { ...c, source_end: cut, fade_out: 0, transition_out: { type: "None" as const } };
  const right = { ...c, id: `${c.id}-split`, source_start: cut, fade_in: 0 };
  const clips = [...p.clips.slice(0, i), left, right, ...p.clips.slice(i + 1)];
  return { project: project(clips, { ...p, clips }), new_id: right.id };
}
