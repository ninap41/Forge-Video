import { beforeEach, describe, expect, it, vi } from "vitest";

// eslint-disable-next-line @typescript-eslint/no-explicit-any
const invoke = vi.fn<(...a: any[]) => Promise<unknown>>(() => Promise.resolve("ok"));
// eslint-disable-next-line @typescript-eslint/no-explicit-any
const listen = vi.fn<(...a: any[]) => Promise<() => void>>(() => Promise.resolve(() => {}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invoke(...a), convertFileSrc: (p: string) => `asset://localhost${p}` }));
vi.mock("@tauri-apps/api/event", () => ({ listen: (...a: unknown[]) => listen(...a) }));
const writeText = vi.fn((_t: string) => Promise.resolve());
vi.mock("@tauri-apps/plugin-clipboard-manager", () => ({ writeText: (t: string) => writeText(t) }));

import { api } from "./tauri";

beforeEach(() => { invoke.mockClear(); listen.mockClear(); });

describe("api → invoke mapping", () => {
  it("uses the Rust command names with camelCase argument keys", async () => {
    await api.projectNew("X");
    await api.projectSave();
    await api.projectSave("/p.forgevideo");
    await api.clipTrim("id", 1, 2);
    await api.clipSplit("id", 500); await api.clipMerge(["a", "b"]);
    await api.clipMove("id", 3);
    await api.clipSetFades("id", 10, 20);
    await api.clipSetTransition("id", { type: "DipToBlack", ms: 300 });
    await api.clipSetVolume("id", 0.5, true);
    await api.clipRename("id", "Intro");
    await api.clipDetachAudio("id");
    await api.clipDetachAudio("id", "t1");
    await api.overlayAdd("/l.png", 100, 1);
    await api.overlaySetPlacement("o", { scale: 0.5, x: 0.1, y: 0.2 });
    await api.audioClipAdd("t", "/m.m4a", 250);
    await api.audioClipSet("c", 0.5, 1, 2, false);
    await api.cacheThumbnails("/v.mp4", 5000);
    await api.exportStart({ destination: "/o.mp4", quality: "High", audio_only: false });
    await api.jobCancel("j");
    await api.ffmpegStatus();
    expect(invoke.mock.calls).toEqual([
      ["project_new", { name: "X" }],
      ["project_save", { path: null }],
      ["project_save", { path: "/p.forgevideo" }],
      ["clip_trim", { id: "id", sourceStart: 1, sourceEnd: 2 }],
      ["clip_split", { id: "id", at: 500 }], ["clip_merge", { ids: ["a", "b"] }],
      ["clip_move", { id: "id", toIndex: 3 }],
      ["clip_set_fades", { id: "id", fadeIn: 10, fadeOut: 20 }],
      ["clip_set_transition", { id: "id", transition: { type: "DipToBlack", ms: 300 } }],
      ["clip_set_volume", { id: "id", volume: 0.5, muted: true }],
      ["clip_rename", { id: "id", name: "Intro" }],
      ["clip_detach_audio", { id: "id", trackId: null }],
      ["clip_detach_audio", { id: "id", trackId: "t1" }],
      ["overlay_add", { path: "/l.png", at: 100, layer: 1 }],
      ["overlay_set_placement", { id: "o", placement: { scale: 0.5, x: 0.1, y: 0.2 } }],
      ["audio_clip_add", { trackId: "t", path: "/m.m4a", at: 250 }],
      ["audio_clip_set", { id: "c", volume: 0.5, fadeIn: 1, fadeOut: 2, muted: false }],
      ["cache_thumbnails", { path: "/v.mp4", durationMs: 5000 }],
      ["export_start", { settings: { destination: "/o.mp4", quality: "High", audio_only: false }, texts: [] }],
      ["job_cancel", { jobId: "j" }],
      ["ffmpeg_status"],
    ]);
  });

  it("rounds float milliseconds so Rust's integer args never reject a playhead from the video clock", async () => {
    await api.clipSplit("id", 1516.6666666666667);
    await api.audioClipSplit("id", 1516.6666666666667);
    await api.overlaySplit("id", 0.4);
    await api.clipTrim("id", 999.5, 4000.49);
    await api.overlayMove("o", 2999.999, 1);
    await api.audioClipAdd("t", "/m.m4a", 250.2);
    expect(invoke.mock.calls.map((c) => c[1])).toEqual([
      { id: "id", at: 1517 }, { id: "id", at: 1517 }, { id: "id", at: 0 },
      { id: "id", sourceStart: 1000, sourceEnd: 4000 }, { id: "o", at: 3000, layer: 1 }, { trackId: "t", path: "/m.m4a", at: 250 },
    ]);
  });

  it("covers every remaining command once", async () => {
    await api.projectGet(); await api.projectOpen("/p"); await api.setAspect("Square1x1"); await api.setCrop({ scale: 2, x: 0, y: 1 }); await api.setVideoMuted(true); await api.setVideoVolume(0.5); await api.setCaptionsEnabled(false);
    await api.mediaImport("/v.mp4"); await api.clipDelete("id"); await api.clipInsert("/v.mp4", 0);
    await api.poolAdd("/a"); await api.poolRemove("p");
    await api.overlayMove("o", 1, 0); await api.overlayLayerAdd(); await api.overlayLayerRemove(1); await api.overlayTrim("o", 0, 1); await api.overlaySplit("o", 1); await api.overlayDelete("o"); await api.overlaySetFades("o", 1, 2); await api.overlaySetAudio("o", 0.5, true); await api.overlaySetOpacity("o", 0.5); await api.overlayLayerSetAudio(1, true, 0.5);
    await api.audioTrackAdd("SFX"); await api.audioTrackUpdate("t", "x", true, 0.5); await api.audioTrackRemove("t");
    await api.audioClipMove("c", "t", 1); await api.audioClipTrim("c", 0, 1); await api.audioClipSplit("c", 1); await api.audioClipDelete("c");
    await api.cacheWaveform("/v.mp4"); await api.exportPlan({ destination: "/o", quality: "Draft", audio_only: true });
    expect(invoke.mock.calls.map((c) => c[0])).toEqual([
      "project_get", "project_open", "project_set_aspect", "project_set_crop", "project_set_video_muted", "project_set_video_volume", "project_set_captions_enabled", "media_import", "clip_delete", "clip_insert",
      "pool_add", "pool_remove",
      "overlay_move", "overlay_layer_add", "overlay_layer_remove", "overlay_trim", "overlay_split", "overlay_delete", "overlay_set_fades", "overlay_set_audio", "overlay_set_opacity", "overlay_layer_set_audio",
      "audio_track_add", "audio_track_update", "audio_track_remove",
      "audio_clip_move", "audio_clip_trim", "audio_clip_split", "audio_clip_delete",
      "cache_waveform", "export_plan",
    ]);
  });

  it("maps the AI mode commands", async () => {
    await api.aiStatus(); await api.aiClaudeLogin(); await api.aiClaudeLogout(); await api.aiInstall(); await api.aiTranscribe(); await api.aiFindHighlights();
    await api.cueSetText("c", "fixed"); await api.highlightDelete("h"); await api.highlightApply("h");
    expect(invoke.mock.calls).toEqual([
      ["ai_status"], ["ai_claude_login"], ["ai_claude_logout"], ["ai_install"], ["ai_transcribe"], ["ai_find_highlights"],
      ["cue_set_text", { id: "c", text: "fixed" }], ["highlight_delete", { id: "h" }], ["highlight_apply", { id: "h" }],
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
