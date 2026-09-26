import { beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { baseProject, clip, media, music, project, type MockApi } from "../test/fixtures";

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
  api.cacheThumbnails.mockImplementation(() => Promise.resolve({ clip_id: "x", interval_ms: 200, files: ["/t/0001.jpg", "/t/0002.jpg"] }));
  api.cacheWaveform.mockImplementation(() => Promise.resolve({ bucket_ms: 10, peaks: [1, 2, 3] }));
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
    expect(api.cacheThumbnails).toHaveBeenCalledWith("n1");
    expect(api.cacheThumbnails).toHaveBeenCalledWith("n2");
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
    await s.updateMusic(music({ volume: 0.9 }));
    expect(api.clipTrim).toHaveBeenCalledWith("a", 100, 2000);
    expect(api.clipMove).toHaveBeenCalledWith("a", 1);
    expect(api.clipSetFades).toHaveBeenCalledWith("a", 10, 20);
    expect(api.clipSetTransition).toHaveBeenCalledWith("a", { type: "CrossDissolve", ms: 500 });
    expect(api.clipSetVolume).toHaveBeenCalledWith("a", 0.2, true);
    expect(api.setAspect).toHaveBeenCalledWith("Shorts9x16");
    expect(api.setCrop).toHaveBeenCalledWith({ scale: 2, x: 0.1, y: 0.9 });
    expect(api.musicUpdate).toHaveBeenCalledWith(expect.objectContaining({ volume: 0.9 }));
    expect(s.dirty).toBe(true);
  });

  it("setMusic fetches the bed's waveform once", async () => {
    const s = useProjectStore();
    const withMusic = project([clip({ id: "a" })], { music: music() });
    api.musicSet.mockImplementation(() => Promise.resolve(withMusic));
    await s.setMusic("/audio/bed.m4a");
    await flush();
    expect(api.musicSet).toHaveBeenCalledWith("/audio/bed.m4a");
    expect(s.waveforms["/audio/bed.m4a"]).toEqual([1, 2, 3]);
    await s.setMusic("/audio/bed.m4a");
    await flush();
    expect(api.cacheWaveform).toHaveBeenCalledTimes(1);
    api.musicSet.mockImplementation(() => Promise.resolve(base));
    await s.setMusic(null);
    expect(s.project?.music).toBeNull();
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
