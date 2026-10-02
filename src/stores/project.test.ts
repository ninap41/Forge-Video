import { beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { audioClip, audioTrack, baseProject, clip, fakeSplit, media, overlay, poolItem, project, stillMedia, textClip, type MockApi, loopFx } from "../test/fixtures";

vi.mock("../api/tauri", async () => {
  const f = await import("../test/fixtures");
  return { api: f.mockApi(f.baseProject()) };
});
import { api as apiModule } from "../api/tauri";
const api = apiModule as unknown as MockApi;

import { useProjectStore } from "./project";

const base = baseProject();

const flush = () => new Promise((r) => setTimeout(r, 0));

beforeEach(() => {
  setActivePinia(createPinia());
  for (const v of Object.values(api)) if (typeof v === "function" && "mockClear" in v) (v as ReturnType<typeof vi.fn>).mockClear();
  api.cacheThumbnails.mockImplementation(() => Promise.resolve({ path: "x", interval_ms: 200, files: ["/t/0001.jpg", "/t/0002.jpg"] }));
  api.cacheWaveform.mockImplementation(() => Promise.resolve({ bucket_ms: 10, peaks: [1, 2, 3] }));
});

describe("notify", () => {
  it("shows a notice, replaces an earlier one, and auto-clears", () => {
    vi.useFakeTimers();
    const s = useProjectStore();
    expect(s.notice).toBeNull();
    s.notify("first");
    s.notify("second", 1000);
    expect(s.notice).toBe("second");
    vi.advanceTimersByTime(999);
    expect(s.notice).toBe("second");
    vi.advanceTimersByTime(1);
    expect(s.notice).toBeNull();
    s.notify("x"); s.notify(null);
    expect(s.notice).toBeNull();
    vi.useRealTimers();
  });

  it("poolAdd skips files already in the pool and notifies, including duplicates Rust folds away", async () => {
    const s = useProjectStore();
    const pooled = project([], { pool: [poolItem({ id: "p", path: "/music/bed.m4a" })] });
    s.project = pooled;
    api.poolAdd.mockResolvedValue(pooled);
    await s.poolAdd(["/music/bed.m4a"]);
    expect(api.poolAdd).not.toHaveBeenCalled();
    expect(s.notice).toBe("bed.m4a is already in the media pool");
    s.notify(null);
    // A symlink to the same file: the store cannot tell, Rust returns the pool unchanged.
    await s.poolAdd(["/links/bed.m4a"]);
    expect(api.poolAdd).toHaveBeenCalledWith("/links/bed.m4a");
    expect(s.notice).toBe("bed.m4a is already in the media pool");
    s.notify(null);
    const grown = project([], { pool: [...pooled.pool, poolItem({ id: "q", path: "/music/new.m4a" })] });
    api.poolAdd.mockResolvedValue(grown);
    await s.poolAdd(["/music/new.m4a"]);
    expect(s.pool).toHaveLength(2);
    expect(s.notice).toBeNull();
  });
});

describe("detachAudio", () => {
  it("calls the command and selects the new audio clip", async () => {
    const s = useProjectStore();
    s.project = baseProject();
    const after = project(baseProject().clips.map((c, i) => (i === 0 ? { ...c, muted: true } : c)), {
      audio_tracks: [audioTrack([audioClip({ id: "detached", source: "/v/a.mp4" })], { id: "t9" })],
    });
    api.clipDetachAudio.mockResolvedValue(after);
    await s.detachAudio("a");
    expect(api.clipDetachAudio).toHaveBeenCalledWith("a", null);
    expect(s.clips[0].muted).toBe(true);
    expect(s.selected).toEqual({ kind: "audio", id: "detached", trackId: "t9" });
    api.clipDetachAudio.mockRejectedValue(new Error("clip has no audio"));
    await s.detachAudio("b", "t9");
    expect(api.clipDetachAudio).toHaveBeenLastCalledWith("b", "t9");
    expect(s.error).toContain("no audio");
  });
});

describe("derived state", () => {
  it("starts empty", () => {
    const s = useProjectStore();
    expect(s.project).toBeNull(); expect(s.clips).toEqual([]); expect(s.duration).toBe(0);
    expect(s.selectedClip).toBeNull(); expect(s.selectedIndex).toBe(-1); expect(s.current).toBeNull();
    expect(s.dirty).toBe(false); expect(s.playing).toBe(false);
  });

  it("load hydrates without marking dirty and warms media caches", async () => {
    const s = useProjectStore();
    await s.load();
    await flush();
    expect(s.clips.map((c) => c.id)).toEqual(["a", "b"]);
    expect(s.duration).toBe(9000);
    expect(s.dirty).toBe(false);
    expect(api.cacheThumbnails).toHaveBeenCalledTimes(2);
    expect(api.cacheWaveform).toHaveBeenCalledTimes(2);
    expect(s.thumbs[base.clips[0].source]).toEqual({ intervalMs: 200, urls: ["asset://localhost/t/0001.jpg", "asset://localhost/t/0002.jpg"] });
    expect(s.waveforms[base.clips[0].source]).toEqual([1, 2, 3]);
  });

  it("does not request a waveform for silent clips and does not refetch cached media", async () => {
    const silent = project([clip({ id: "s", media: media({ has_audio: false }) })]);
    api.projectGet.mockImplementationOnce(() => Promise.resolve(silent));
    const s = useProjectStore();
    await s.load();
    await flush();
    expect(api.cacheWaveform).not.toHaveBeenCalled();
    expect(api.cacheThumbnails).toHaveBeenCalledTimes(1);
    api.projectGet.mockImplementationOnce(() => Promise.resolve(silent));
    await s.load();
    await flush();
    expect(api.cacheThumbnails).toHaveBeenCalledTimes(1);
  });

  it("current mirrors timeline::locate", async () => {
    const s = useProjectStore();
    await s.load();
    s.playhead = 0;
    expect(s.current).toMatchObject({ index: 0, sourceMs: 0 });
    s.playhead = 4999;
    expect(s.current).toMatchObject({ index: 0, sourceMs: 4999 });
    s.playhead = 5000;
    expect(s.current).toMatchObject({ index: 1, sourceMs: 0 });
    s.playhead = 8999;
    expect(s.current).toMatchObject({ index: 1, sourceMs: 3999 });
  });

  it("seek clamps into [0, duration-1]", async () => {
    const s = useProjectStore();
    s.seek(500);
    expect(s.playhead).toBe(0);
    await s.load();
    s.seek(-10); expect(s.playhead).toBe(0);
    s.seek(2000); expect(s.playhead).toBe(2000);
    s.seek(99_999); expect(s.playhead).toBe(8999);
  });

  it("select / selectedClip / selectedIndex", async () => {
    const s = useProjectStore();
    await s.load();
    s.select("b");
    expect(s.selectedClip?.id).toBe("b");
    expect(s.selectedIndex).toBe(1);
    s.select(null);
    expect(s.selectedClip).toBeNull();
  });

  it("⇧-select extends within a row, toggles, and Merge joins the pieces and selects the result", async () => {
    const s = useProjectStore();
    await s.load();
    s.select("a");
    s.select("b", true);
    expect(s.selectedAll.map((x) => x.id)).toEqual(["a", "b"]);
    s.select("a", true); // the primary cannot be toggled off
    expect(s.selectedAll.map((x) => x.id)).toEqual(["a", "b"]);
    s.select("b", true); // toggles the extra off
    expect(s.selectedAll.map((x) => x.id)).toEqual(["a"]);
    s.select({ kind: "overlay", id: "o" }, true); // another row replaces the selection
    expect(s.selectedAll).toEqual([{ kind: "overlay", id: "o" }]);
    s.select("a"); s.select("b", true);
    s.select("b"); // a plain click collapses to one
    expect(s.selectedAll.map((x) => x.id)).toEqual(["b"]);
    s.select("a", true);
    const merged = { ...s.project!, clips: [s.project!.clips[0]] };
    api.clipMerge.mockImplementationOnce(() => Promise.resolve({ project: merged, new_id: "a" }));
    await s.mergeSelected();
    expect(api.clipMerge).toHaveBeenCalledWith(["b", "a"]);
    expect(s.clips.map((c) => c.id)).toEqual(["a"]);
    expect(s.selectedAll.map((x) => x.id)).toEqual(["a"]);
    expect(s.dirty).toBe(true);
    await s.mergeSelected(); // one clip selected: nothing to do
    expect(api.clipMerge).toHaveBeenCalledTimes(1);
  });
});

describe("mutations", () => {
  it("apply marks dirty, drops a selection that vanished and clamps the playhead", async () => {
    const s = useProjectStore();
    await s.load();
    s.select("b");
    s.playhead = 8000;
    api.clipDelete.mockImplementationOnce(() => Promise.resolve(project([clip({ id: "a" })])));
    await s.deleteClip("b");
    expect(s.clips.map((c) => c.id)).toEqual(["a"]);
    expect(s.selectedClipId).toBeNull();
    expect(s.playhead).toBe(4999);
    expect(s.dirty).toBe(true);
    expect(api.clipDelete).toHaveBeenCalledWith("b");
  });

  it("importMedia imports sequentially, selects the last clip and caches it", async () => {
    const s = useProjectStore();
    const after1 = project([clip({ id: "n1" })]);
    const after2 = project([clip({ id: "n1" }), clip({ id: "n2" })]);
    api.mediaImport.mockImplementationOnce(() => Promise.resolve(after1)).mockImplementationOnce(() => Promise.resolve(after2));
    await s.importMedia(["/v/1.mp4", "/v/2.mp4"]);
    expect(api.mediaImport.mock.calls).toEqual([["/v/1.mp4"], ["/v/2.mp4"]]);
    expect(s.selectedClipId).toBe("n2");
    expect(s.dirty).toBe(true);
    await flush();
    expect(api.cacheThumbnails).toHaveBeenCalledWith(after2.clips[0].source, 5000);
    expect(api.cacheThumbnails).toHaveBeenCalledWith(after2.clips[1].source, 5000);
  });

  it("importMedia does not select anything when the file went to the pool only", async () => {
    const s = useProjectStore();
    await s.load();
    await flush();
    const withPool = { ...baseProject(), pool: [poolItem({ media: stillMedia(), path: "/images/logo.png" })] };
    api.mediaImport.mockImplementationOnce(() => Promise.resolve(withPool));
    await s.importMedia(["/images/logo.png"]);
    expect(s.selected).toBeNull();
    expect(s.pool).toHaveLength(1);
    await flush();
    expect(api.cacheThumbnails).toHaveBeenCalledWith("/images/logo.png", 5000);
    expect(api.cacheWaveform).toHaveBeenCalledTimes(2); // no waveform for a still
  });

  it("errors are captured on the store instead of throwing, and busy flags reset", async () => {
    const s = useProjectStore();
    const spy = vi.spyOn(console, "error").mockImplementation(() => {});
    api.mediaImport.mockImplementationOnce(() => Promise.reject("media error: no video stream"));
    await s.importMedia(["/bad.txt"]);
    expect(s.error).toBe("media error: no video stream");
    expect(s.project).toBeNull();
    expect(s.busy.import).toBe(false);
    spy.mockRestore();
    // next successful call clears the error
    api.mediaImport.mockImplementationOnce(() => Promise.resolve(base));
    await s.importMedia(["/ok.mp4"]);
    expect(s.error).toBeNull();
  });

  it("splitAtPlayhead uses the clip under the playhead and selects the new half", async () => {
    const s = useProjectStore();
    await s.load();
    s.playhead = 7000;
    const after = project([clip({ id: "a" }), clip({ id: "b", source_end: 2000 }), clip({ id: "b2", source_start: 2000, source_end: 4000 })]);
    api.clipSplit.mockImplementationOnce(() => Promise.resolve({ project: after, new_id: "b2" }));
    await s.splitAtPlayhead();
    expect(api.clipSplit).toHaveBeenCalledWith("b", 7000);
    expect(s.selectedClipId).toBe("b2");
    expect(s.clips).toHaveLength(3);
  });

  it("splitAtPlayhead is a no-op on an empty timeline", async () => {
    const s = useProjectStore();
    await s.splitAtPlayhead();
    expect(api.clipSplit).not.toHaveBeenCalled();
    expect(s.error).toBe("Nothing to split");
  });

  it("⌘T really splits the clip under the playhead into two contiguous halves", async () => {
    const s = useProjectStore();
    api.clipSplit.mockImplementation((id: string, at: number) => Promise.resolve(fakeSplit(s.project!, id, at)));
    await s.load();
    const before = s.duration;
    s.select("a");
    s.playhead = 2000;
    await s.splitAtPlayhead();
    expect(s.error).toBeNull();
    expect(s.clips.map((c) => c.id)).toEqual(["a", "a-split", "b"]);
    const [left, right, b] = s.clips;
    expect([left.source_start, left.source_end]).toEqual([0, 2000]);
    expect([right.source_start, right.source_end]).toEqual([2000, 5000]);
    expect(right.source).toBe(left.source);
    expect([left.timeline_start, right.timeline_start, b.timeline_start]).toEqual([0, 2000, 5000]);
    expect(s.duration).toBe(before);
    expect(s.selected).toEqual({ kind: "clip", id: "a-split" });
    expect(s.dirty).toBe(true);
    // split the new half again: three pieces of "a"
    s.playhead = 3500;
    await s.splitAtPlayhead();
    expect(s.clips.map((c) => [c.id, c.source_start, c.source_end])).toEqual([
      ["a", 0, 2000], ["a-split", 2000, 3500], ["a-split-split", 3500, 5000], ["b", 0, 4000],
    ]);
    expect(s.duration).toBe(before);
  });

  it("⌘T with a float playhead (from the video clock) still reaches Rust as a whole number", async () => {
    const s = useProjectStore();
    api.clipSplit.mockImplementation((id: string, at: number) => Promise.resolve(fakeSplit(s.project!, id, Math.round(at))));
    await s.load();
    s.playhead = 1516.6666666666667;
    await s.splitAtPlayhead();
    expect(api.clipSplit).toHaveBeenCalledWith("a", 1516.6666666666667);
    expect(s.clips.map((c) => [c.source_start, c.source_end])).toEqual([[0, 1517], [1517, 5000], [0, 4000]]);
    expect(s.error).toBeNull();
  });

  it("⌘T uses the clip under the playhead even when another V1 clip is selected", async () => {
    const s = useProjectStore();
    api.clipSplit.mockImplementation((id: string, at: number) => Promise.resolve(fakeSplit(s.project!, id, at)));
    await s.load();
    s.select("a");
    s.playhead = 7000; // inside b (5000..9000)
    await s.splitAtPlayhead();
    expect(api.clipSplit).toHaveBeenCalledWith("b", 7000);
    expect(s.clips.map((c) => [c.id, c.timeline_start])).toEqual([["a", 0], ["b", 5000], ["b-split", 7000]]);
  });

  it("⌘T on a selected overlay/audio clip that is not under the playhead falls back to V1", async () => {
    const s = useProjectStore();
    const ov = overlay({ id: "ov1", timeline_start: 6000, source_end: 2000 });
    const ac = audioClip({ id: "ac1", timeline_start: 6000, source_end: 2000 });
    const p = { ...baseProject(), overlays: [ov], audio_tracks: [audioTrack([ac], { id: "t1" })] };
    api.projectGet.mockImplementationOnce(() => Promise.resolve(p));
    api.clipSplit.mockImplementation((id: string, at: number) => Promise.resolve(fakeSplit(s.project!, id, at)));
    await s.load();
    s.select({ kind: "overlay", id: "ov1" });
    s.playhead = 2000;
    await s.splitAtPlayhead();
    expect(api.overlaySplit).not.toHaveBeenCalled();
    expect(api.clipSplit).toHaveBeenCalledWith("a", 2000);
    expect(s.clips).toHaveLength(3);
    s.select({ kind: "audio", id: "ac1", trackId: "t1" });
    s.playhead = 7000;
    await s.splitAtPlayhead();
    expect(api.audioClipSplit).toHaveBeenCalledWith("ac1", 7000);
  });

  it("⌘T at a clip boundary reports a clear error instead of calling Rust", async () => {
    const s = useProjectStore();
    await s.load();
    s.playhead = 0;
    await s.splitAtPlayhead();
    expect(api.clipSplit).not.toHaveBeenCalled();
    expect(s.error).toBe("Move the playhead inside a clip to split it");
    s.playhead = 5000; // exactly where b starts: not inside a, and at b's left edge
    await s.splitAtPlayhead();
    expect(api.clipSplit).not.toHaveBeenCalled();
    // Rust rejections still land on the store's error field
    s.playhead = 4950;
    api.clipSplit.mockImplementationOnce(() => Promise.reject("invalid edit: split would create a clip that is too short"));
    const spy = vi.spyOn(console, "error").mockImplementation(() => {});
    await s.splitAtPlayhead();
    spy.mockRestore();
    expect(s.error).toContain("too short");
    expect(s.clips).toHaveLength(2);
  });

  it("forwards the simple edits with their arguments", async () => {
    const s = useProjectStore();
    await s.load();
    await s.trim("a", 100, 2000);
    await s.moveClip("a", 1);
    await s.setFades("a", 10, 20);
    await s.setTransition("a", { type: "CrossDissolve", ms: 500 });
    await s.setVolume("a", 0.2, true);
    await s.setAspect("Shorts9x16");
    await s.setCrop({ scale: 2, x: 0.1, y: 0.9 });
    await s.setVideoMuted(true);
    expect(api.setVideoMuted).toHaveBeenCalledWith(true);
    await s.setVideoVolume(0.3);
    expect(api.setVideoVolume).toHaveBeenCalledWith(0.3);
    await s.setCaptionsEnabled(false);
    expect(api.setCaptionsEnabled).toHaveBeenCalledWith(false);
    expect(api.clipTrim).toHaveBeenCalledWith("a", 100, 2000);
    expect(api.clipMove).toHaveBeenCalledWith("a", 1);
    expect(api.clipSetFades).toHaveBeenCalledWith("a", 10, 20);
    expect(api.clipSetTransition).toHaveBeenCalledWith("a", { type: "CrossDissolve", ms: 500 });
    expect(api.clipSetVolume).toHaveBeenCalledWith("a", 0.2, true);
    expect(api.setAspect).toHaveBeenCalledWith("Shorts9x16");
    expect(api.setCrop).toHaveBeenCalledWith({ scale: 2, x: 0.1, y: 0.9 });
    expect(s.dirty).toBe(true);
  });

  it("forwards overlay, audio-track and pool edits", async () => {
    const s = useProjectStore();
    await s.load();
    await s.overlayMove("o", 100, 1); await s.overlayLayerAdd(); await s.overlayLayerRemove(1); await s.overlayTrim("o", 0, 2000); await s.overlaySetFades("o", 1, 2); await s.overlaySetAudio("o", 0.5, true); await s.overlaySetOpacity("o", 0.5); await s.overlayLayerSetAudio(1, true, 0.5);
    await s.overlaySetPlacement("o", { scale: 0.5, x: 0.1, y: 0.2 });
    await s.audioTrackAdd("SFX"); await s.audioTrackUpdate("t", "Narration", true, 0.5); await s.audioTrackRemove("t");
    await s.renameClip("c", "Intro");
    expect(api.clipRename).toHaveBeenCalledWith("c", "Intro");
    await s.audioClipTrim("c", 1, 500); await s.audioClipSet("c", 0.5, 10, 20, true);
    await s.poolAdd(["/a.mp3", "/b.png"]); await s.poolRemove("p"); await s.insertClip("/v.mp4", 1);
    expect(api.overlayMove).toHaveBeenCalledWith("o", 100, 1);
    expect(api.overlayLayerAdd).toHaveBeenCalled();
    expect(api.overlayLayerRemove).toHaveBeenCalledWith(1);
    expect(api.overlayTrim).toHaveBeenCalledWith("o", 0, 2000);
    expect(api.overlaySetFades).toHaveBeenCalledWith("o", 1, 2);
    expect(api.overlaySetAudio).toHaveBeenCalledWith("o", 0.5, true);
    expect(api.overlaySetOpacity).toHaveBeenCalledWith("o", 0.5);
    expect(api.overlayLayerSetAudio).toHaveBeenCalledWith(1, true, 0.5);
    expect(api.overlaySetPlacement).toHaveBeenCalledWith("o", { scale: 0.5, x: 0.1, y: 0.2 });
    expect(api.audioTrackAdd).toHaveBeenCalledWith("SFX");
    expect(api.audioTrackUpdate).toHaveBeenCalledWith("t", "Narration", true, 0.5);
    expect(api.audioTrackRemove).toHaveBeenCalledWith("t");
    expect(api.audioClipTrim).toHaveBeenCalledWith("c", 1, 500);
    expect(api.audioClipSet).toHaveBeenCalledWith("c", 0.5, 10, 20, true);
    expect(api.poolAdd.mock.calls).toEqual([["/a.mp3"], ["/b.png"]]);
    expect(api.poolRemove).toHaveBeenCalledWith("p");
    expect(api.clipInsert).toHaveBeenCalledWith("/v.mp4", 1);
    expect(s.dirty).toBe(true);
  });

  it("overlayAdd / audioClipAdd select what was placed and cache its media", async () => {
    const s = useProjectStore();
    await s.load();
    const ov = overlay({ id: "ov1", timeline_start: 1000, source: "/images/logo.png" });
    api.overlayAdd.mockImplementationOnce(() => Promise.resolve(project([clip({ id: "a" })], { overlays: [ov] })));
    await s.overlayAdd("/images/logo.png", 1000);
    expect(api.overlayAdd).toHaveBeenCalledWith("/images/logo.png", 1000, 0);
    expect(s.selected).toEqual({ kind: "overlay", id: "ov1" });
    expect(s.selectedOverlay?.id).toBe("ov1");
    const ac = audioClip({ id: "ac1", timeline_start: 500, source: "/audio/bed.m4a" });
    const track = audioTrack([ac], { id: "t1" });
    api.audioClipAdd.mockImplementationOnce(() => Promise.resolve(project([clip({ id: "a" })], { audio_tracks: [track] })));
    await s.audioClipAdd("t1", "/audio/bed.m4a", 500);
    expect(api.audioClipAdd).toHaveBeenCalledWith("t1", "/audio/bed.m4a", 500);
    expect(s.selected).toEqual({ kind: "audio", id: "ac1", trackId: "t1" });
    expect(s.selectedAudio?.track.label).toBe("Music");
    await flush();
    expect(api.cacheWaveform).toHaveBeenCalledWith("/audio/bed.m4a");
    expect(api.cacheThumbnails).not.toHaveBeenCalledWith("/audio/bed.m4a", expect.anything());
  });

  it("currentOverlays and activeAudioClips follow the playhead and resolve track mute", async () => {
    const s = useProjectStore();
    const ov = overlay({ id: "ov1", timeline_start: 1000, source_end: 2000 });
    const ov2 = overlay({ id: "ov2", timeline_start: 2600, source_end: 1000, layer: 1 });
    const a1 = audioClip({ id: "a1", timeline_start: 0, source_end: 3000 });
    const a2 = audioClip({ id: "a2", timeline_start: 2500, source_end: 1000, source_start: 0 });
    const p = project([clip({ id: "a" })], { overlays: [ov, ov2], overlay_layers: 2, audio_tracks: [audioTrack([a1], { id: "t1" }), audioTrack([a2], { id: "t2", muted: true })] });
    api.projectGet.mockImplementationOnce(() => Promise.resolve(p));
    await s.load();
    expect(s.overlayLayers).toBe(2);
    s.playhead = 500;
    expect(s.currentOverlays).toEqual([]);
    expect(s.activeAudioClips.map((x) => x.clip.id)).toEqual(["a1"]);
    s.playhead = 2700;
    expect(s.currentOverlays.map((o) => [o.clip.id, o.sourceMs])).toEqual([["ov1", 1700], ["ov2", 100]]);
    expect(s.activeAudioClips.map((x) => [x.clip.id, x.silent, x.sourceMs])).toEqual([["a1", false, 2700], ["a2", true, 200]]);
    s.playhead = 3700;
    expect(s.currentOverlays).toEqual([]);
  });

  it("splitAtPlayhead and deleteSelected dispatch on the selection kind", async () => {
    const s = useProjectStore();
    const ov = overlay({ id: "ov1", timeline_start: 0, source_end: 4000 });
    const ac = audioClip({ id: "ac1", timeline_start: 0, source_end: 4000 });
    const p = project([clip({ id: "a" })], { overlays: [ov], audio_tracks: [audioTrack([ac], { id: "t1" })] });
    api.projectGet.mockImplementationOnce(() => Promise.resolve(p));
    await s.load();
    s.playhead = 2000;
    s.select({ kind: "overlay", id: "ov1" });
    const afterOv = project([clip({ id: "a" })], { overlays: [{ ...ov, source_end: 2000 }, overlay({ id: "ov2", timeline_start: 2000, source_start: 2000, source_end: 4000 })], audio_tracks: p.audio_tracks });
    api.overlaySplit.mockImplementationOnce(() => Promise.resolve({ project: afterOv, new_id: "ov2" }));
    await s.splitAtPlayhead();
    expect(api.overlaySplit).toHaveBeenCalledWith("ov1", 2000);
    expect(api.clipSplit).not.toHaveBeenCalled();
    expect(s.selected).toEqual({ kind: "overlay", id: "ov2" });
    s.select({ kind: "audio", id: "ac1", trackId: "t1" });
    api.audioClipSplit.mockImplementationOnce(() => Promise.resolve({ project: afterOv, new_id: "zzz" }));
    await s.splitAtPlayhead();
    expect(api.audioClipSplit).toHaveBeenCalledWith("ac1", 2000);
    expect(s.selected).toBeNull(); // the new id is not in the echoed project, so the selection is dropped
    s.select({ kind: "audio", id: "ac1", trackId: "t1" });
    await s.deleteSelected();
    expect(api.audioClipDelete).toHaveBeenCalledWith("ac1");
    s.select({ kind: "overlay", id: "ov1" });
    await s.deleteSelected();
    expect(api.overlayDelete).toHaveBeenCalledWith("ov1");
    s.select("a");
    await s.deleteSelected();
    expect(api.clipDelete).toHaveBeenCalledWith("a");
  });

  it("text track: add selects the title, edits forward, ⌘T/⌫ dispatch on it, rasters cover titles inside V1 only", async () => {
    const s = useProjectStore();
    const p = project([clip({ id: "a" })]);
    api.projectGet.mockImplementationOnce(() => Promise.resolve(p));
    await s.load();
    const t1 = textClip({ id: "t1", timeline_start: 1000, duration: 2000, fade_in: 500 });
    api.textAdd.mockImplementationOnce(() => Promise.resolve({ ...p, texts: [t1] }));
    await s.textAdd(1000);
    expect(api.textAdd).toHaveBeenCalledWith("Title", 1000, 0);
    expect(s.selected).toEqual({ kind: "text", id: "t1" });
    expect(s.selectedText?.id).toBe("t1");
    expect(api.cacheThumbnails).toHaveBeenCalledTimes(1); // only the V1 clip: titles have no media
    s.playhead = 1250;
    expect(s.currentTexts).toEqual([{ clip: t1, opacity: 0.5 }]);
    s.playhead = 3500;
    expect(s.currentTexts).toEqual([]);
    for (const k of ["textMove", "textTrim", "textSetFades", "textSetPosition", "textSetStyle", "textLayerAdd", "textLayerRemove"] as const) api[k].mockImplementation(() => Promise.resolve({ ...p, texts: [t1] }));
    await s.textMove("t1", 5, 1);
    await s.textTrim("t1", 3000);
    await s.textSetFades("t1", 1, 2);
    await s.textSetPosition("t1", 0.1, 0.2);
    await s.textSetStyle("t1", { ...t1.style, color: "#ff0000" });
    expect(api.textMove).toHaveBeenCalledWith("t1", 5, 1);
    await s.textLayerAdd(); await s.textLayerRemove(1);
    expect(api.textLayerAdd).toHaveBeenCalled();
    expect(api.textLayerRemove).toHaveBeenCalledWith(1);
    expect(api.textTrim).toHaveBeenCalledWith("t1", 3000);
    expect(api.textSetFades).toHaveBeenCalledWith("t1", 1, 2);
    expect(api.textSetPosition).toHaveBeenCalledWith("t1", 0.1, 0.2);
    expect(api.textSetStyle).toHaveBeenCalledWith("t1", expect.objectContaining({ color: "#ff0000" }));
    s.playhead = 2000;
    s.select({ kind: "text", id: "t1" });
    api.textSplit.mockImplementationOnce(() => Promise.resolve({ project: { ...p, texts: [{ ...t1, duration: 1000 }, textClip({ id: "t2", timeline_start: 2000, duration: 1000 })] }, new_id: "t2" }));
    await s.splitAtPlayhead();
    expect(api.textSplit).toHaveBeenCalledWith("t1", 2000);
    expect(s.selected).toEqual({ kind: "text", id: "t2" });
    await s.deleteSelected();
    expect(api.textDelete).toHaveBeenCalledWith("t2");
    // fonts load once
    await s.loadFonts(); await s.loadFonts();
    expect(api.systemFonts).toHaveBeenCalledTimes(1);
    expect(s.fonts).toContain("Impact");
    // rasters: one per title that starts inside V1 (5 s), at the requested size
    s.project = { ...p, texts: [t1, textClip({ id: "late", timeline_start: 9000 })] };
    const r = s.textRasters(1080, 1920);
    expect(r).toEqual([{ id: "t1", png: "iVBORw0KGgo=" }]);
  });

  it("range selection: sorted, clamped, cleared; loops pin, select, play and export it", async () => {
    const s = useProjectStore();
    const p = project([clip({ id: "a" })]); // 5 s
    api.projectGet.mockImplementationOnce(() => Promise.resolve(p));
    await s.load();
    expect(s.range).toBeNull();
    s.setRange({ start: 4000, end: 1000 });
    expect(s.range).toEqual({ start: 1000, end: 4000 });
    s.setRange({ start: 3000, end: 99_000 });
    expect(s.range).toEqual({ start: 3000, end: 5000 });
    s.setRange({ start: 1000, end: 1050 });
    expect(s.range).toBeNull();
    // loop toggle follows the range
    s.setRange({ start: 1000, end: 3000 });
    s.loopOn = true;
    expect(s.loopRange).toEqual({ start: 1000, end: 3000 });
    s.setRange(null);
    expect(s.loopOn).toBe(false);
    expect(s.loopRange).toBeNull();
    // playRange: seeks, loops, plays; pressed again, pauses
    s.playRange({ start: 2000, end: 4000 });
    expect([s.playhead, s.loopOn, s.playing, s.range]).toEqual([2000, true, true, { start: 2000, end: 4000 }]);
    s.playRange({ start: 2000, end: 4000 });
    expect(s.playing).toBe(false);
    // pinning: Rust returns the project with the loop; it becomes the active one
    const l = loopFx({ id: "l1", name: "Hook", start: 2000, end: 4000 });
    api.loopAdd.mockImplementationOnce(() => Promise.resolve({ ...p, loops: [l] }));
    await s.loopAdd("Hook");
    expect(api.loopAdd).toHaveBeenCalledWith("Hook", 2000, 4000);
    expect(s.activeLoopId).toBe("l1");
    expect(s.activeLoop).toEqual(l);
    s.setRange({ start: 0, end: 1000 });
    expect(s.activeLoopId).toBeNull();
    s.selectLoop("l1");
    expect(s.range).toEqual({ start: 2000, end: 4000 });
    expect(s.activeLoopId).toBe("l1");
    expect(s.loopOn).toBe(true);
    s.playhead = 3000;
    s.togglePlay();
    expect([s.playing, s.playhead]).toEqual([true, 2000]);
    s.togglePlay();
    expect(s.playing).toBe(false);
    s.playing = true;
    s.exportLoop("l1");
    expect([s.exportOpen, s.playing, s.activeLoopId]).toEqual([true, false, "l1"]);
    // a project without the loop clears the active id; a shorter project clamps the range
    api.loopRemove.mockImplementationOnce(() => Promise.resolve({ ...p, clips: [clip({ id: "a", source_end: 3000 })], loops: [] }));
    await s.loopRemove("l1");
    expect(api.loopRemove).toHaveBeenCalledWith("l1");
    expect(s.activeLoopId).toBeNull();
    expect(s.range).toEqual({ start: 2000, end: 3000 });
    // loopsOpen persists
    s.loopsOpen = false;
    await new Promise((r) => setTimeout(r, 0));
    expect(localStorage.getItem("forgevideo.loopsOpen")).toBe("0");
  });

  it("remembers the last project path: save and open store it, launch reopens it, New forgets it", async () => {
    const s = useProjectStore();
    expect(s.lastProjectPath).toBeNull();
    api.projectSave.mockResolvedValueOnce("/work/reel.forgevideo");
    await s.save("/work/reel.forgevideo");
    expect(s.lastProjectPath).toBe("/work/reel.forgevideo");
    expect(localStorage.getItem("forgevideo.lastProject")).toBe("/work/reel.forgevideo");
    await s.open("/work/other.forgevideo");
    expect(s.lastProjectPath).toBe("/work/other.forgevideo");
    // a fresh store (new launch) reopens that file instead of asking Rust for the blank project
    setActivePinia(createPinia());
    const s2 = useProjectStore();
    api.projectGet.mockClear(); api.projectOpen.mockClear();
    await s2.load();
    expect(api.projectOpen).toHaveBeenCalledWith("/work/other.forgevideo");
    expect(api.projectGet).not.toHaveBeenCalled();
    expect(s2.dirty).toBe(false);
    // a file that no longer opens is forgotten and the blank project loads
    api.projectOpen.mockRejectedValueOnce("no such file");
    setActivePinia(createPinia());
    const s3 = useProjectStore();
    await s3.load();
    expect(api.projectGet).toHaveBeenCalled();
    expect(s3.lastProjectPath).toBeNull();
    expect(localStorage.getItem("forgevideo.lastProject")).toBe("");
    await s3.open("/work/reel.forgevideo");
    await s3.newProject();
    expect(s3.lastProjectPath).toBeNull();
  });

  it("poolView and timelineHeight persist to localStorage", async () => {
    const s = useProjectStore();
    s.poolView = "list";
    s.timelineHeight = 420;
    await flush();
    expect(localStorage.getItem("forgevideo.poolView")).toBe("list");
    expect(localStorage.getItem("forgevideo.timelineHeight")).toBe("420");
    setActivePinia(createPinia());
    const s2 = useProjectStore();
    expect(s2.poolView).toBe("list");
    expect(s2.timelineHeight).toBe(420);
    localStorage.clear();
  });

  it("save clears dirty only on success; open and newProject reset dirty", async () => {
    const s = useProjectStore();
    await s.load();
    await s.trim("a", 0, 1000);
    expect(s.dirty).toBe(true);
    api.projectSave.mockImplementationOnce(() => Promise.reject("invalid edit: no save path"));
    const spy = vi.spyOn(console, "error").mockImplementation(() => {});
    expect(await s.save()).toBeUndefined();
    expect(s.dirty).toBe(true);
    spy.mockRestore();
    expect(await s.save("/p.forgevideo")).toBe("/saved.forgevideo");
    expect(api.projectSave).toHaveBeenLastCalledWith("/p.forgevideo");
    expect(s.dirty).toBe(false);

    await s.trim("a", 0, 1000);
    await s.open("/other.forgevideo");
    expect(api.projectOpen).toHaveBeenCalledWith("/other.forgevideo");
    expect(s.dirty).toBe(false);

    s.playhead = 1000;
    await s.trim("a", 0, 1000);
    await s.newProject();
    expect(api.projectNew).toHaveBeenCalledWith("Untitled");
    expect(s.dirty).toBe(false);
    expect(s.playhead).toBe(0);
  });
});
