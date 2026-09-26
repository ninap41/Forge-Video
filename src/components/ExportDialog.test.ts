import { beforeEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { clip, project, type MockApi } from "../test/fixtures";
import type { JobDone, JobError, JobProgress } from "../types/project";

vi.mock("../api/tauri", async () => {
  const f = await import("../test/fixtures");
  return { api: f.mockApi(f.project([])) };
});
import { api as apiModule } from "../api/tauri";
const api = apiModule as unknown as MockApi;
const saveDialog = vi.fn();
const reveal = vi.fn();
vi.mock("@tauri-apps/plugin-dialog", () => ({ save: (...a: unknown[]) => saveDialog(...(a as [])) }));
vi.mock("@tauri-apps/plugin-opener", () => ({ revealItemInDir: (...a: unknown[]) => reveal(...(a as [])) }));

import ExportDialog from "./ExportDialog.vue";
import { useProjectStore } from "../stores/project";

const flush = () => new Promise((r) => setTimeout(r, 0));
let progressCb: (e: JobProgress) => void, doneCb: (e: JobDone) => void, errorCb: (e: JobError) => void;

function setup(open = true) {
  setActivePinia(createPinia());
  const store = useProjectStore();
  store.project = project([clip({ id: "a" })], { name: "Reel" });
  const w = mount(ExportDialog, { props: { open } });
  return { store, w };
}
const btn = (w: ReturnType<typeof mount>, t: string) => w.findAll("button").find((b) => b.text() === t)!;

beforeEach(() => {
  for (const v of Object.values(api)) if (typeof v === "function" && "mockClear" in v) (v as ReturnType<typeof vi.fn>).mockClear();
  api.exportPlan.mockImplementation(() => Promise.resolve({ strategy: "StreamCopy", duration_ms: 5000, output: [1920, 1080], destination: "/o.mp4", reasons: [], args: [] }));
  api.onJobProgress.mockImplementation((cb: (e: JobProgress) => void) => { progressCb = cb; return Promise.resolve(() => {}); });
  api.onJobDone.mockImplementation((cb: (e: JobDone) => void) => { doneCb = cb; return Promise.resolve(() => {}); });
  api.onJobError.mockImplementation((cb: (e: JobError) => void) => { errorCb = cb; return Promise.resolve(() => {}); });
  saveDialog.mockReset(); reveal.mockReset();
});

describe("ExportDialog", () => {
  it("renders nothing when closed and asks for a plan when opened", async () => {
    const { w } = setup(false);
    expect(w.find("h2").exists()).toBe(false);
    expect(api.exportPlan).not.toHaveBeenCalled();
    await w.setProps({ open: true });
    await flush();
    expect(w.find("h2").text()).toBe("Export");
    expect(api.exportPlan).toHaveBeenCalledWith({ destination: "/tmp/forge-video-preview.mp4", quality: "Standard", audio_only: false });
  });

  it("explains the stream-copy strategy", async () => {
    const { w } = setup();
    await flush();
    expect(w.text()).toContain("Fast trim · no re-encode");
    expect(w.text()).toContain("0:05.0 · 1920×1080");
    expect(w.text()).toContain("nearest keyframe");
  });

  it("lists re-encode reasons and the render badge", async () => {
    api.exportPlan.mockImplementation(() => Promise.resolve({ strategy: "HardwareEncode", duration_ms: 9000, output: [1080, 1920], destination: "/o.mp4", reasons: ["fades", "aspect ratio change"], args: [] }));
    const { w } = setup();
    await flush();
    expect(w.text()).toContain("Render · VideoToolbox H.264");
    expect(w.text()).toContain("Re-encoding because of: fades, aspect ratio change.");
    expect(w.text()).toContain("1080×1920");
  });

  it("audio-only hides quality, changes the badge and the picker extension", async () => {
    api.exportPlan.mockImplementation((s: { audio_only: boolean }) => Promise.resolve({ strategy: s.audio_only ? "AudioOnly" : "StreamCopy", duration_ms: 5000, output: [0, 0], destination: "/o", reasons: [], args: [] }));
    const { w } = setup();
    await flush();
    await w.find("input[type=checkbox]").setValue(true);
    await flush();
    expect(w.text()).toContain("Audio only · AAC");
    expect(w.text()).not.toContain("×");
    expect(btn(w, "High").attributes("disabled")).toBeDefined();
    saveDialog.mockResolvedValueOnce("/out/Reel.m4a");
    await btn(w, "Choose…").trigger("click");
    await flush();
    expect(saveDialog).toHaveBeenCalledWith({ defaultPath: "Reel.m4a", filters: [{ name: "M4A", extensions: ["m4a"] }] });
    expect(w.text()).toContain("Reel.m4a");
    expect(api.exportPlan).toHaveBeenLastCalledWith(expect.objectContaining({ destination: "/out/Reel.m4a", audio_only: true }));
  });

  it("re-plans when quality changes and shows plan errors", async () => {
    const { w } = setup();
    await flush();
    await btn(w, "High").trigger("click");
    await flush();
    expect(api.exportPlan).toHaveBeenLastCalledWith(expect.objectContaining({ quality: "High" }));
    api.exportPlan.mockImplementation(() => Promise.reject("export failed: timeline is empty"));
    await btn(w, "Draft").trigger("click");
    await flush();
    expect(w.text()).toContain("export failed: timeline is empty");
    expect(btn(w, "Export").attributes("disabled")).toBeDefined();
  });

  it("runs a job: picks a destination, shows progress, then the done screen", async () => {
    const { store, w } = setup();
    await flush();
    store.playing = true;
    saveDialog.mockResolvedValueOnce("/out/Reel.mp4");
    await btn(w, "Export").trigger("click");
    await flush();
    expect(saveDialog).toHaveBeenCalledWith(expect.objectContaining({ defaultPath: "Reel.mp4" }));
    expect(api.exportStart).toHaveBeenCalledWith({ destination: "/out/Reel.mp4", quality: "Standard", audio_only: false });
    expect(store.playing).toBe(false);
    expect(w.text()).toContain("0%");
    expect(btn(w, "Cancel")).toBeTruthy();
    expect(btn(w, "✕").attributes("disabled")).toBeDefined();

    progressCb({ job_id: "other", kind: "export", progress: 0.9, message: null });
    await w.vm.$nextTick();
    expect(w.text()).toContain("0%");
    progressCb({ job_id: "job-1", kind: "export", progress: 0.42, message: null });
    await w.vm.$nextTick();
    expect(w.text()).toContain("42%");
    expect((w.find(".bg-accent.transition-\\[width\\]").element as HTMLElement).style.width).toBe("42%");

    await btn(w, "Cancel").trigger("click");
    expect(api.jobCancel).toHaveBeenCalledWith("job-1");

    doneCb({ job_id: "job-1", kind: "export", result: { destination: "/out/Reel.mp4", strategy: "StreamCopy" } });
    await w.vm.$nextTick();
    expect(w.text()).toMatch(/Done in \d+\.\ds/);
    expect(w.text()).toContain("/out/Reel.mp4");
    await btn(w, "Reveal in Finder").trigger("click");
    expect(reveal).toHaveBeenCalledWith("/out/Reel.mp4");
    await btn(w, "Close").trigger("click");
    expect(w.emitted("close")).toHaveLength(1);
  });

  it("shows job errors and lets the user retry", async () => {
    const { w } = setup();
    await flush();
    saveDialog.mockResolvedValueOnce("/out/Reel.mp4");
    await btn(w, "Export").trigger("click");
    await flush();
    errorCb({ job_id: "job-1", kind: "export", error: "export failed: ffmpeg exited with 1" });
    await w.vm.$nextTick();
    expect(w.text()).toContain("ffmpeg exited with 1");
    expect(btn(w, "Export")).toBeTruthy();
    await btn(w, "Export").trigger("click");
    await flush();
    expect(saveDialog).toHaveBeenCalledTimes(1);
    expect(api.exportStart).toHaveBeenCalledTimes(2);
  });

  it("cancelling the save dialog aborts, and exportStart failures are shown", async () => {
    const { w } = setup();
    await flush();
    saveDialog.mockResolvedValueOnce(null);
    await btn(w, "Export").trigger("click");
    await flush();
    expect(api.exportStart).not.toHaveBeenCalled();
    saveDialog.mockResolvedValueOnce("/out/x.mp4");
    api.exportStart.mockImplementationOnce(() => Promise.reject("export failed: boom"));
    await btn(w, "Export").trigger("click");
    await flush();
    expect(w.text()).toContain("export failed: boom");
  });

  it("clicking the backdrop closes unless a job is running", async () => {
    const { w } = setup();
    await flush();
    await w.find(".fixed").trigger("pointerdown");
    expect(w.emitted("close")).toHaveLength(1);
    saveDialog.mockResolvedValueOnce("/out/x.mp4");
    await btn(w, "Export").trigger("click");
    await flush();
    await w.find(".fixed").trigger("pointerdown");
    expect(w.emitted("close")).toHaveLength(1);
  });
});
