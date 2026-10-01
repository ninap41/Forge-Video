import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { audioClip, audioTrack, clip, media, overlay, project, resolveWith, stillMedia, textClip, type MockApi } from "../test/fixtures";

vi.mock("../api/tauri", async () => {
  const f = await import("../test/fixtures");
  return { api: f.mockApi(f.project([])) };
});
import { api as apiModule } from "../api/tauri";
const api = apiModule as unknown as MockApi;

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

beforeEach(() => { for (const v of Object.values(api)) if (typeof v === "function" && "mockClear" in v) (v as ReturnType<typeof vi.fn>).mockClear(); });

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
    const mute = w.find("button[data-muted]");
    expect(mute.classes()).toContain("text-accent");
    await mute.trigger("click");
    expect(api.clipSetVolume).toHaveBeenLastCalledWith("a", 1.5, true);
    await w.vm.$nextTick();
    expect(w.find("button[data-muted]").attributes("data-muted")).toBe("true");
    const calls = api.clipSetVolume.mock.calls.length;
    const { w: silent } = setup(project([clip({ id: "a", media: media({ has_audio: false }) })]), "a");
    expect(silent.find("button[data-muted]").attributes("disabled")).toBeDefined();
    await silent.find("button[data-muted]").trigger("click");
    expect(api.clipSetVolume).toHaveBeenCalledTimes(calls); // silent clips ignore the toggle
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

describe("Inspector · overlay and audio clips", () => {
  it("shows nothing special when only a V1 clip is selected", () => {
    const { w } = setup(undefined, "a");
    expect(w.text()).toContain("Overlays (V2) and audio clips show their controls here");
    expect(w.text()).not.toContain("Reset placement");
  });

  it("edits the selected overlay: length, size, fades, placement reset, delete", async () => {
    const ov = overlay({ id: "ov1", media: stillMedia(), timeline_start: 1000, source_end: 5000, placement: { scale: 0.35, x: 0.85, y: 0.85 } });
    const { store, w } = setup(project([clip({ id: "a" })], { overlays: [ov] }));
    store.select({ kind: "overlay", id: "ov1" });
    await w.vm.$nextTick();
    expect(w.text()).toContain("Still image");
    expect(w.text()).toContain("35%");
    const section = w.findAll("section").find((s) => s.text().includes("Overlay"))!;
    const sliders = section.findAll("input[type=range]");
    await sliders[0].setValue("8000"); await sliders[0].trigger("change");
    expect(api.overlayTrim).toHaveBeenCalledWith("ov1", 0, 8000);
    await sliders[1].setValue("0.5"); await sliders[1].trigger("change");
    expect(api.overlaySetPlacement).toHaveBeenCalledWith("ov1", { scale: 0.5, x: 0.85, y: 0.85 });
    await sliders[2].setValue("0.3"); await sliders[2].trigger("change");
    expect(api.overlaySetOpacity).toHaveBeenCalledWith("ov1", 0.3);
    expect(w.find("[data-testid=overlay-opacity]").exists()).toBe(true);
    await sliders[3].setValue("500"); await sliders[3].trigger("change");
    expect(api.overlaySetFades).toHaveBeenCalledWith("ov1", 500, 0);
    await byText(w, "Reset placement").trigger("click");
    expect(api.overlaySetPlacement).toHaveBeenLastCalledWith("ov1", { scale: 0.35, x: 0.85, y: 0.85 });
    await byText(w, "Delete").trigger("click");
    expect(api.overlayDelete).toHaveBeenCalledWith("ov1");
  });

  it("edits the selected title: text, font, size, colour, backdrop, length, fades, position, delete", async () => {
    const t = textClip({ id: "t1", timeline_start: 1000, duration: 4000, style: { text: "Hi", font: "Quicksand", size: 0.08, color: "#ffffff", backdrop: null } });
    const { store, w } = setup(project([clip({ id: "a" })], { texts: [t] }));
    store.select({ kind: "text", id: "t1" });
    await w.vm.$nextTick(); await w.vm.$nextTick();
    const section = w.find("[data-testid=text-section]");
    expect(section.text()).toContain("Title");
    expect(api.systemFonts).toHaveBeenCalledTimes(1);
    await w.vm.$nextTick();
    expect(w.findAll("[data-testid=text-font] option").map((o) => o.text())).toEqual(["Quicksand", "Orbit", "Helvetica Neue", "Impact"]);
    const ta = w.find("[data-testid=text-text]");
    await ta.setValue("Hello\nworld"); await ta.trigger("change");
    expect(api.textSetStyle).toHaveBeenLastCalledWith("t1", { text: "Hello\nworld", font: "Quicksand", size: 0.08, color: "#ffffff", backdrop: null });
    await w.find("[data-testid=text-font]").setValue("Impact");
    expect(api.textSetStyle).toHaveBeenLastCalledWith("t1", expect.objectContaining({ font: "Impact" }));
    const size = w.find("[data-testid=text-size]");
    await size.setValue("0.2"); await size.trigger("change");
    expect(api.textSetStyle).toHaveBeenLastCalledWith("t1", expect.objectContaining({ size: 0.2 }));
    const color = w.find("[data-testid=text-color]");
    await color.setValue("#ff0000"); await color.trigger("change");
    expect(api.textSetStyle).toHaveBeenLastCalledWith("t1", expect.objectContaining({ color: "#ff0000" }));
    // backdrop: off by default; ticking it sends colour + opacity, which then edit in place
    expect(w.find("[data-testid=text-backdrop-color]").exists()).toBe(false);
    await w.find("[data-testid=text-backdrop]").setValue(true);
    expect(api.textSetStyle).toHaveBeenLastCalledWith("t1", expect.objectContaining({ backdrop: { color: "#000000", opacity: 0.65 } }));
    const bd = w.find("[data-testid=text-backdrop-color]");
    await bd.setValue("#0000ff"); await bd.trigger("change");
    const bo = w.find("[data-testid=text-backdrop-opacity]");
    await bo.setValue("0.3"); await bo.trigger("change");
    expect(api.textSetStyle).toHaveBeenLastCalledWith("t1", expect.objectContaining({ backdrop: { color: "#0000ff", opacity: 0.3 } }));
    await w.find("[data-testid=text-backdrop]").setValue(false);
    expect(api.textSetStyle).toHaveBeenLastCalledWith("t1", expect.objectContaining({ backdrop: null }));
    const len = w.find("[data-testid=text-length]");
    await len.setValue("8000"); await len.trigger("change");
    expect(api.textTrim).toHaveBeenCalledWith("t1", 8000);
    const fades = section.findAll("input[type=range]").filter((i) => i.attributes("max") === "4000");
    expect(fades).toHaveLength(2);
    await fades[1].setValue("500"); await fades[1].trigger("change");
    expect(api.textSetFades).toHaveBeenCalledWith("t1", 0, 500);
    await byText(w, "Reset position").trigger("click");
    expect(api.textSetPosition).toHaveBeenCalledWith("t1", 0.5, 0.85);
    await byText(w, "Delete").trigger("click");
    expect(api.textDelete).toHaveBeenCalledWith("t1");
  });

  it("gives a video overlay volume and mute; stills and silent files get neither", async () => {
    const ov = overlay({ id: "ov1", media: media({ has_audio: true }), source_end: 3000, volume: 0.8 });
    const { store, w } = setup(project([clip({ id: "a" })], { overlays: [ov] }));
    store.select({ kind: "overlay", id: "ov1" });
    await w.vm.$nextTick();
    expect(w.text()).toContain("Clip audio on · row V2 on");
    const vol = w.find("[data-testid=overlay-volume]");
    expect((vol.element as HTMLInputElement).value).toBe("0.8");
    await vol.setValue("0.4"); await vol.trigger("change");
    expect(api.overlaySetAudio).toHaveBeenCalledWith("ov1", 0.4, false);
    await w.find("button[aria-label='Mute overlay']").trigger("click");
    expect(api.overlaySetAudio).toHaveBeenLastCalledWith("ov1", 0.4, true);
    store.project = { ...store.project!, overlays: [overlay({ id: "ov1", media: media({ has_audio: false }), source_end: 3000 })] };
    await w.vm.$nextTick();
    expect(w.find("[data-testid=overlay-volume]").exists()).toBe(false);
    expect(w.text()).toContain("Video (no sound)");
  });

  it("shows the captions track with its on/off switch when selected", async () => {
    const p = project([clip({ id: "a", source: "/v/a.mp4" })], { transcripts: [{ source: "/v/a.mp4", cues: [{ id: "c1", start: 0, end: 1000, text: "hi" }] }] });
    const { store, w } = setup(p);
    expect(w.find("[data-testid=captions-section]").exists()).toBe(false);
    store.select({ kind: "captions", id: "captions" });
    await w.vm.$nextTick();
    const section = w.find("[data-testid=captions-section]");
    expect(section.text()).toContain("1 captions");
    expect(section.text()).toContain("Captions on");
    expect(w.text()).toContain("Captions track selected.");
    const box = w.find("[data-testid=captions-enabled]");
    expect((box.element as HTMLInputElement).checked).toBe(true);
    await box.setValue(false);
    expect(api.setCaptionsEnabled).toHaveBeenCalledWith(false);
    store.project = { ...store.project!, captions_enabled: false };
    await w.vm.$nextTick();
    expect(w.find("[data-testid=captions-section]").text()).toContain("Captions off");
    // deleting the selection is a no-op for the captions track
    await store.deleteSelected();
    expect(api.clipDelete).not.toHaveBeenCalled();
  });

  it("edits the selected audio clip and its track mute", async () => {
    const ac = audioClip({ id: "ac1", source: "/audio/lofi.mp3", volume: 0.5, timeline_start: 2000 });
    const { store, w } = setup(project([clip({ id: "a" })], { audio_tracks: [audioTrack([ac], { id: "t1", label: "Narration" })] }));
    store.select({ kind: "audio", id: "ac1", trackId: "t1" });
    await w.vm.$nextTick();
    expect(w.text()).toContain("Audio · Narration");
    expect(w.text()).toContain("lofi.mp3");
    expect(w.text()).toContain("50%");
    const section = w.findAll("section").find((s) => s.text().includes("Audio · Narration"))!;
    const sliders = section.findAll("input[type=range]");
    await sliders[0].setValue("0.8"); await sliders[0].trigger("change");
    expect(api.audioClipSet).toHaveBeenCalledWith("ac1", 0.8, 0, 0, false);
    await sliders[2].setValue("1000"); await sliders[2].trigger("change");
    expect(api.audioClipSet).toHaveBeenLastCalledWith("ac1", 0.8, 0, 1000, false);
    const toggles = section.findAll("button[data-muted]");
    expect(toggles[0].classes()).toContain("text-accent");
    await toggles[0].trigger("click");
    expect(api.audioClipSet).toHaveBeenLastCalledWith("ac1", 0.8, 0, 1000, true);
    await w.vm.$nextTick();
    expect(section.findAll("button[data-muted]")[0].classes()).toContain("text-muted");
    await toggles[1].trigger("click");
    expect(api.audioTrackUpdate).toHaveBeenCalledWith("t1", "Narration", true, 1);
    await byText(w, "Delete").trigger("click");
    expect(api.audioClipDelete).toHaveBeenCalledWith("ac1");
  });
});
