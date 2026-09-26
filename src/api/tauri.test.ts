import { beforeEach, describe, expect, it, vi } from "vitest";

// eslint-disable-next-line @typescript-eslint/no-explicit-any
const invoke = vi.fn<(...a: any[]) => Promise<unknown>>(() => Promise.resolve("ok"));
// eslint-disable-next-line @typescript-eslint/no-explicit-any
const listen = vi.fn<(...a: any[]) => Promise<() => void>>(() => Promise.resolve(() => {}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invoke(...a), convertFileSrc: (p: string) => `asset://localhost${p}` }));
vi.mock("@tauri-apps/api/event", () => ({ listen: (...a: unknown[]) => listen(...a) }));

import { api } from "./tauri";

beforeEach(() => { invoke.mockClear(); listen.mockClear(); });

describe("api → invoke mapping", () => {
  it("uses the Rust command names with camelCase argument keys", async () => {
    await api.projectNew("X");
    await api.projectSave();
    await api.projectSave("/p.forgevideo");
    await api.clipTrim("id", 1, 2);
    await api.clipSplit("id", 500);
    await api.clipMove("id", 3);
    await api.clipSetFades("id", 10, 20);
    await api.clipSetTransition("id", { type: "DipToBlack", ms: 300 });
    await api.clipSetVolume("id", 0.5, true);
    await api.musicSet(null);
    await api.cacheThumbnails("cid");
    await api.exportStart({ destination: "/o.mp4", quality: "High", audio_only: false });
    await api.jobCancel("j");
    await api.ffmpegStatus();
    expect(invoke.mock.calls).toEqual([
      ["project_new", { name: "X" }],
      ["project_save", { path: null }],
      ["project_save", { path: "/p.forgevideo" }],
      ["clip_trim", { id: "id", sourceStart: 1, sourceEnd: 2 }],
      ["clip_split", { id: "id", at: 500 }],
      ["clip_move", { id: "id", toIndex: 3 }],
      ["clip_set_fades", { id: "id", fadeIn: 10, fadeOut: 20 }],
      ["clip_set_transition", { id: "id", transition: { type: "DipToBlack", ms: 300 } }],
      ["clip_set_volume", { id: "id", volume: 0.5, muted: true }],
      ["music_set", { path: null }],
      ["cache_thumbnails", { clipId: "cid" }],
      ["export_start", { settings: { destination: "/o.mp4", quality: "High", audio_only: false } }],
      ["job_cancel", { jobId: "j" }],
      ["ffmpeg_status"],
    ]);
  });

  it("covers every remaining command once", async () => {
    await api.projectGet(); await api.projectOpen("/p"); await api.setAspect("Square1x1"); await api.setCrop({ scale: 2, x: 0, y: 1 });
    await api.mediaImport("/v.mp4"); await api.clipDelete("id"); await api.musicUpdate({ source: "/m", duration_ms: 1, timeline_start: 0, trim_start: 0, trim_end: 1, volume: 1, fade_in: 0, fade_out: 0, muted: false });
    await api.cacheWaveform("/v.mp4"); await api.exportPlan({ destination: "/o", quality: "Draft", audio_only: true });
    expect(invoke.mock.calls.map((c) => c[0])).toEqual([
      "project_get", "project_open", "project_set_aspect", "project_set_crop", "media_import", "clip_delete", "music_update", "cache_waveform", "export_plan",
    ]);
  });

  it("subscribes to job events and unwraps payloads", async () => {
    const cb = vi.fn();
    await api.onJobProgress(cb); await api.onJobDone(cb); await api.onJobError(cb);
    expect(listen.mock.calls.map((c) => c[0])).toEqual(["job://progress", "job://done", "job://error"]);
    const handler = listen.mock.calls[0][1] as (e: { payload: unknown }) => void;
    handler({ payload: { job_id: "j", progress: 0.5 } });
    expect(cb).toHaveBeenCalledWith({ job_id: "j", progress: 0.5 });
  });

  it("assetUrl goes through convertFileSrc", () => {
    expect(api.assetUrl("/Users/x/a.mp4")).toBe("asset://localhost/Users/x/a.mp4");
  });
});
