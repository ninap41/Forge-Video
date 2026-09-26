import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { clip, media, music, project, resolveWith, type MockApi } from "../test/fixtures";

vi.mock("../api/tauri", async () => {
  const f = await import("../test/fixtures");
  return { api: f.mockApi(f.project([])) };
});
import { api as apiModule } from "../api/tauri";
const api = apiModule as unknown as MockApi;
const dialogOpen = vi.fn();
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: (...a: unknown[]) => dialogOpen(...(a as [])) }));

import Inspector from "./Inspector.vue";
import { useProjectStore } from "../stores/project";

function setup(p = project([clip({ id: "a" }), clip({ id: "b" })]), selected: string | null = null) {
  setActivePinia(createPinia());
  const store = useProjectStore();
  store.project = p;
  store.selectedClipId = selected;
  resolveWith(api, p);
  mounted.push(mount(Inspector));
  return { store, w: mounted[mounted.length - 1] };
}
const mounted: ReturnType<typeof mount>[] = [];
afterEach(() => { mounted.splice(0).forEach((w) => w.unmount()); });
const byText = (w: ReturnType<typeof mount>, t: string) => w.findAll("button").find((b) => b.text().includes(t))!;

beforeEach(() => { for (const v of Object.values(api)) if (typeof v === "function" && "mockClear" in v) (v as ReturnType<typeof vi.fn>).mockClear(); dialogOpen.mockReset(); });

describe("Inspector · output", () => {
  it("lists the four presets and highlights the active one", () => {
    const { w } = setup();
    const btns = w.findAll("section")[0].findAll("button");
    expect(btns.slice(0, 4).map((b) => b.text())).toEqual([
      expect.stringContaining("YouTube"), expect.stringContaining("Shorts / Reels"), expect.stringContaining("Square"), expect.stringContaining("LinkedIn"),
    ]);
    expect(btns[0].classes()).toContain("border-accent");
    expect(btns[1].classes()).not.toContain("border-accent");
    expect(btns[1].text()).toContain("1080×1920");
  });

  it("clicking a preset calls setAspect; framing zoom commits on change; reset clears", async () => {
    const { w } = setup();
    await byText(w, "Shorts").trigger("click");
    expect(api.setAspect).toHaveBeenCalledWith("Shorts9x16");
    const zoom = w.findAll("section")[0].find("input[type=range]");
    await zoom.setValue("2.5");
    await zoom.trigger("change");
    expect(api.setCrop).toHaveBeenCalledWith({ scale: 2.5, x: 0.5, y: 0.5 });
    await byText(w, "Reset framing").trigger("click");
    expect(api.setCrop).toHaveBeenLastCalledWith({ scale: 1, x: 0.5, y: 0.5 });
  });
});

describe("Inspector · clip", () => {
  it("prompts when nothing is selected", () => {
    const { w } = setup();
    expect(w.text()).toContain("Select a clip on the timeline.");
  });

  it("shows metadata and in/out points for the selected clip", () => {
    const { w } = setup(project([clip({ id: "a", source: "/v/interview.mov", source_start: 1000, source_end: 3500, media: media({ fps: { num: 30000, den: 1001 }, codec: "hevc" }) })]), "a");
    expect(w.text()).toContain("interview.mov");
    expect(w.text()).toContain("1920×1080 · 29.97 fps · hevc");
    expect(w.text()).toContain("In 0:01.0 · Out 0:03.5 · 0:02.5");
  });

  it("fade sliders are capped at 5 s or the clip length and commit on change", async () => {
    const { w } = setup(project([clip({ id: "a", source_end: 2000 }), clip({ id: "b" })]), "a");
    const sliders = w.findAll("section")[1].findAll("input[type=range]");
    expect(sliders[0].attributes("max")).toBe("2000");
    await sliders[0].setValue("500");
    await sliders[0].trigger("change");
    expect(api.clipSetFades).toHaveBeenCalledWith("a", 500, 0);
    await sliders[1].setValue("250");
    await sliders[1].trigger("change");
    expect(api.clipSetFades).toHaveBeenLastCalledWith("a", 500, 250);
    const { w: long } = setup(project([clip({ id: "a", media: media({ duration_ms: 60000 }) })]), "a");
    expect(long.findAll("section")[1].find("input[type=range]").attributes("max")).toBe("5000");
  });

  it("transition buttons commit, expose a length slider, and are disabled on the last clip", async () => {
    const { w } = setup(undefined, "a");
    await byText(w, "Dissolve").trigger("click");
    expect(api.clipSetTransition).toHaveBeenCalledWith("a", { type: "CrossDissolve", ms: 500 });
    expect(w.text()).toContain("Length");
    const len = w.findAll("label").find((l) => l.text().includes("Length"))!.find("input");
    await len.setValue("1500");
    await len.trigger("change");
    expect(api.clipSetTransition).toHaveBeenLastCalledWith("a", { type: "CrossDissolve", ms: 1500 });
    await byText(w, "Cut").trigger("click");
    expect(api.clipSetTransition).toHaveBeenLastCalledWith("a", { type: "None" });

    const { w: last } = setup(undefined, "b");
    expect(byText(last, "Dissolve").attributes("disabled")).toBeDefined();
    expect(last.text()).toContain("Last clip has nothing to transition into.");
  });

  it("volume + mute commit together; disabled for silent clips", async () => {
    const { w } = setup(undefined, "a");
    const vol = w.findAll("label").find((l) => l.text().includes("Volume"))!.find("input");
    await vol.setValue("1.5");
    await vol.trigger("change");
    expect(api.clipSetVolume).toHaveBeenCalledWith("a", 1.5, false);
    const mute = w.find("input[type=checkbox]");
    await mute.setValue(true);
    expect(api.clipSetVolume).toHaveBeenLastCalledWith("a", 1.5, true);
    const { w: silent } = setup(project([clip({ id: "a", media: media({ has_audio: false }) })]), "a");
    expect(silent.find("input[type=checkbox]").attributes("disabled")).toBeDefined();
  });

  it("split and delete act on the selected clip; split needs the playhead inside it", async () => {
    const { store, w } = setup(undefined, "b");
    store.playhead = 1000; // inside clip a, not b
    await w.vm.$nextTick();
    const split = () => byText(w, "Split at playhead");
    expect(split().attributes("disabled")).toBeDefined();
    store.playhead = 6000;
    await w.vm.$nextTick();
    expect(split().attributes("disabled")).toBeUndefined();
    await split().trigger("click");
    expect(api.clipSplit).toHaveBeenCalledWith("b", 6000);
    await w.vm.$nextTick();
    store.selectedClipId = "b"; // the echoed project keeps b, but the split result selects "new"
    await w.vm.$nextTick();
    await byText(w, "Delete").trigger("click");
    expect(api.clipDelete).toHaveBeenCalledWith("b");
  });

  it("local slider state follows the selected clip", async () => {
    const { store, w } = setup(project([clip({ id: "a", fade_in: 300, volume: 0.4 }), clip({ id: "b", fade_in: 0, volume: 1 })]), "a");
    expect(w.text()).toContain("0.30s");
    expect(w.text()).toContain("40%");
    store.selectedClipId = "b";
    await w.vm.$nextTick();
    expect(w.text()).toContain("0.00s");
    expect(w.text()).toContain("100%");
  });
});

describe("Inspector · music", () => {
  it("offers to add a track and opens an audio picker", async () => {
    const { w } = setup();
    expect(w.text()).toContain("Add music track…");
    dialogOpen.mockResolvedValueOnce("/audio/bed.m4a");
    await byText(w, "Add music track").trigger("click");
    await Promise.resolve();
    expect(dialogOpen).toHaveBeenCalledWith(expect.objectContaining({ multiple: false }));
    expect(api.musicSet).toHaveBeenCalledWith("/audio/bed.m4a");
    dialogOpen.mockResolvedValueOnce(null);
    await byText(w, "Add music track").trigger("click");
    await Promise.resolve();
    expect(api.musicSet).toHaveBeenCalledTimes(1);
  });

  it("edits and removes an existing track", async () => {
    const { w } = setup(project([clip({ id: "a" })], { music: music({ source: "/audio/lofi.mp3", volume: 0.5 }) }));
    expect(w.text()).toContain("lofi.mp3");
    expect(w.text()).toContain("50%");
    const musicSection = w.findAll("section")[2];
    const sliders = musicSection.findAll("input[type=range]");
    await sliders[0].setValue("0.8");
    await sliders[0].trigger("change");
    expect(api.musicUpdate).toHaveBeenCalledWith(expect.objectContaining({ source: "/audio/lofi.mp3", volume: 0.8 }));
    await sliders[1].setValue("2000");
    await sliders[1].trigger("change");
    expect(api.musicUpdate).toHaveBeenLastCalledWith(expect.objectContaining({ timeline_start: 2000 }));
    await musicSection.find("input[type=checkbox]").setValue(true);
    expect(api.musicUpdate).toHaveBeenLastCalledWith(expect.objectContaining({ muted: true }));
    await byText(w, "Remove").trigger("click");
    expect(api.musicSet).toHaveBeenCalledWith(null);
  });
});
