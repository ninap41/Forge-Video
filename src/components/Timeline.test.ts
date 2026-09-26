import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { audioClip, audioTrack, clip, overlay, poolItem, project, resolveWith, stillMedia, audioMedia, type MockApi } from "../test/fixtures";
import FreeBlock from "./FreeBlock.vue";

vi.mock("../api/tauri", async () => {
  const f = await import("../test/fixtures");
  return { api: f.mockApi(f.project([])) };
});
import { api as apiModule } from "../api/tauri";
const api = apiModule as unknown as MockApi;

import Timeline from "./Timeline.vue";
import ClipBlock from "./ClipBlock.vue";
import { useProjectStore } from "../stores/project";
import { resizeAll } from "../test/setup";

function setup(p = project([clip({ id: "a" }), clip({ id: "b", source_end: 4000 })])) {
  setActivePinia(createPinia());
  const store = useProjectStore();
  store.project = p;
  resolveWith(api, p);
  const w = mount(Timeline);
  mounted.push(w);
  resizeAll(1024, 200);
  return { store, w };
}
const mounted: ReturnType<typeof mount>[] = [];
afterEach(() => { mounted.splice(0).forEach((w) => w.unmount()); });
// "Fit" never goes below a 10 s ruler, so a 9 s project is laid out over 10 s.
const PX_PER_MS = (1024 - 48) / 10_000;
const flush = () => new Promise((r) => setTimeout(r, 0));

beforeEach(() => { for (const v of Object.values(api)) if (typeof v === "function" && "mockClear" in v) (v as ReturnType<typeof vi.fn>).mockClear(); });

describe("Timeline", () => {
  it("shows playhead, total, and clip count", async () => {
    const { store, w } = setup();
    expect(w.text()).toContain("0:00.0");
    expect(w.text()).toContain("/ 0:09.0");
    expect(w.text()).toContain("2 clips");
    store.playhead = 1234;
    await w.vm.$nextTick();
    expect(w.text()).toContain("0:01.2");
    const { w: one } = setup(project([clip()]));
    expect(one.text()).toContain("1 clip");
    expect(one.text()).not.toContain("1 clips");
  });

  it("renders one ClipBlock per clip with the shared scale", async () => {
    const { w } = setup();
    await w.vm.$nextTick();
    const blocks = w.findAllComponents(ClipBlock);
    expect(blocks).toHaveLength(2);
    expect(blocks[0].props("pxPerMs")).toBeCloseTo(PX_PER_MS, 6);
    expect(blocks[1].props("index")).toBe(1);
  });

  it("selecting a block updates the store", async () => {
    const { store, w } = setup();
    await w.vm.$nextTick();
    await w.findAllComponents(ClipBlock)[1].trigger("pointerdown");
    expect(store.selectedClipId).toBe("b");
  });

  it("ruler ticks pick a step that keeps ~70px between labels", async () => {
    const { w } = setup();
    await w.vm.$nextTick();
    // px/ms ≈ 0.0976 → 1000ms=97.6px ≥ 70, 500ms=48.8px < 70 → 1 s steps → 0..10 s = 11 ticks
    const ticks = w.findAll(".cursor-text > div");
    expect(ticks).toHaveLength(11);
    expect(ticks[0].text()).toBe("0:00");
    expect(ticks[10].text()).toBe("0:10");
    expect(parseFloat((ticks[1].element as HTMLElement).style.left)).toBeCloseTo(1000 * PX_PER_MS + 24, 3);
  });

  it("empty timeline still shows a 10 s ruler", () => {
    const { w } = setup(project([]));
    expect(w.text()).toContain("0 clips");
    expect(w.findAll(".cursor-text > div").length).toBeGreaterThan(5);
  });

  it("zoom slider changes scale and Fit resets it", async () => {
    const { w } = setup();
    await w.vm.$nextTick();
    const before = w.findAllComponents(ClipBlock)[0].props("pxPerMs");
    await w.find("input[type=range]").setValue("4");
    expect(w.findAllComponents(ClipBlock)[0].props("pxPerMs")).toBeCloseTo(before * 4, 6);
    await w.find("button").trigger("click");
    expect(w.findAllComponents(ClipBlock)[0].props("pxPerMs")).toBeCloseTo(before, 6);
  });

  it("scrubbing the ruler seeks and stops playback", async () => {
    const { store, w } = setup();
    await w.vm.$nextTick();
    store.playing = true;
    const scroller = w.find(".overflow-x-auto").element as HTMLElement;
    scroller.getBoundingClientRect = () => ({ left: 0, top: 0, width: 1024, height: 200, right: 1024, bottom: 200, x: 0, y: 0, toJSON() {} });
    const pxPerMs = PX_PER_MS;
    await w.find(".cursor-text").trigger("pointerdown", { clientX: 24 + 3000 * pxPerMs, pointerId: 1 });
    expect(store.playing).toBe(false);
    expect(store.playhead).toBeCloseTo(3000, 3);
    await w.find(".overflow-x-auto").trigger("pointermove", { clientX: 24 + 6000 * pxPerMs });
    expect(store.playhead).toBeCloseTo(6000, 3);
    await w.find(".overflow-x-auto").trigger("pointerup");
    await w.find(".overflow-x-auto").trigger("pointermove", { clientX: 24 });
    expect(store.playhead).toBeCloseTo(6000, 3);
  });

  it("dragging a trim handle ripples locally and commits a frame-snapped trim on release", async () => {
    const { store, w } = setup();
    await w.vm.$nextTick();
    const scroller = w.find(".overflow-x-auto");
    const pxPerMs = PX_PER_MS;
    const blocks = w.findAllComponents(ClipBlock);
    const endHandle = blocks[0].findAll(".cursor-ew-resize")[1];
    await endHandle.trigger("pointerdown", { clientX: 500, pointerId: 1 });
    await scroller.trigger("pointermove", { clientX: 500 - 1000 * pxPerMs });
    // local override: clip a is now ~4000 long and clip b starts at ~4000
    const shown = w.findAllComponents(ClipBlock).map((b) => b.props("clip"));
    expect(shown[0].source_end).toBeCloseTo(4000, 0);
    expect(shown[1].timeline_start).toBeCloseTo(4000, 0);
    expect(store.clips[0].source_end).toBe(5000);
    await scroller.trigger("pointerup");
    expect(api.clipTrim).toHaveBeenCalledWith("a", 0, 4000);
  });

  it("trim start cannot pass the minimum clip length", async () => {
    const { w } = setup();
    await w.vm.$nextTick();
    const scroller = w.find(".overflow-x-auto");
    const startHandle = w.findAllComponents(ClipBlock)[0].findAll(".cursor-ew-resize")[0];
    await startHandle.trigger("pointerdown", { clientX: 0, pointerId: 1 });
    await scroller.trigger("pointermove", { clientX: 5000 });
    await scroller.trigger("pointerup");
    expect(api.clipTrim).toHaveBeenCalledWith("a", 4900, 5000);
  });

  it("a click without movement is not a move; dragging past the neighbour reorders", async () => {
    const { w } = setup();
    await w.vm.$nextTick();
    const scroller = w.find(".overflow-x-auto");
    const pxPerMs = PX_PER_MS;
    const a = w.findAllComponents(ClipBlock)[0];
    await a.trigger("pointerdown", { clientX: 100, pointerId: 1 });
    await scroller.trigger("pointermove", { clientX: 102 });
    await scroller.trigger("pointerup");
    expect(api.clipMove).not.toHaveBeenCalled();
    await a.trigger("pointerdown", { clientX: 100, pointerId: 1 });
    await scroller.trigger("pointermove", { clientX: 100 + 6000 * pxPerMs });
    expect(w.findAllComponents(ClipBlock)[0].props("clip").timeline_start).toBeCloseTo(6000, 0);
    await scroller.trigger("pointerup");
    expect(api.clipMove).toHaveBeenCalledWith("a", 1);
  });

  it("renders overlay and audio rows with their clips, labels and hints", async () => {
    const ov = overlay({ id: "ov1", timeline_start: 1000, source: "/images/logo.png" });
    const ac = audioClip({ id: "ac1", timeline_start: 2000, source: "/audio/bed.m4a", source_end: 3000 });
    const { w } = setup(project([clip({ id: "a" })], { overlays: [ov], audio_tracks: [audioTrack([ac], { id: "t1", label: "SFX", muted: true })] }));
    await w.vm.$nextTick();
    const blocks = w.findAllComponents(FreeBlock);
    expect(blocks).toHaveLength(2);
    expect(blocks[0].props("kind")).toBe("overlay");
    expect(blocks[0].text()).toContain("logo.png");
    expect(parseFloat((blocks[0].element as HTMLElement).style.left)).toBeCloseTo(1000 * PX_PER_MS, 3);
    expect(blocks[1].props("kind")).toBe("audio");
    expect(blocks[1].text()).toContain("bed.m4a");
    expect(w.text()).toContain("♪ SFX");
    expect(w.text()).toContain("V2 · overlay");
    expect(w.text()).not.toContain("V3 · overlay");
    expect(w.text()).not.toContain("No audio tracks");
    const { w: none } = setup();
    expect(none.text()).toContain("No audio tracks");
    expect(none.text()).toContain("Drop a PNG or video here");
  });

  it("the V1 gutter has a track-level mute that also hides the waveforms", async () => {
    const { store, w } = setup();
    await w.vm.$nextTick();
    const mute = w.find("button[aria-label='Mute video audio']");
    expect(mute.classes()).toContain("text-accent");
    expect(w.findAllComponents(ClipBlock)[0].props("peaks")).toBeUndefined(); // no waveform cached yet in this test
    await mute.trigger("click");
    expect(api.setVideoMuted).toHaveBeenCalledWith(true);
    store.project = { ...store.project!, video_muted: true };
    store.waveforms[store.clips[0].source] = [1, 2, 3];
    await w.vm.$nextTick();
    expect(w.find("button[aria-label='Unmute video audio']").exists()).toBe(true);
    expect(w.findAllComponents(ClipBlock)[0].props("peaks")).toBeUndefined();
    store.project = { ...store.project!, video_muted: false };
    await w.vm.$nextTick();
    expect(w.findAllComponents(ClipBlock)[0].props("peaks")).toEqual([1, 2, 3]);
  });

  it("the gutter adds, mutes, renames and removes audio tracks", async () => {
    const { w } = setup(project([clip({ id: "a" })], { audio_tracks: [audioTrack([], { id: "t1", label: "Music" })] }));
    const gutterButtons = w.findAll("button");
    await gutterButtons.find((b) => b.text() === "+ Track")!.trigger("click");
    expect(api.audioTrackAdd).toHaveBeenCalledWith("Audio");
    const mute = w.find("button[aria-label='Mute track']");
    expect(mute.classes()).toContain("text-accent");
    expect(mute.attributes("data-muted")).toBe("false");
    await mute.trigger("click");
    expect(api.audioTrackUpdate).toHaveBeenCalledWith("t1", "Music", true);
    await w.find("button[title='Remove track']").trigger("click");
    expect(api.audioTrackRemove).toHaveBeenCalledWith("t1");
    await w.find("button[title='Rename Music']").trigger("click");
    const input = w.find("input[list=track-presets]");
    await input.setValue("Narration");
    await input.trigger("change");
    expect(api.audioTrackUpdate).toHaveBeenLastCalledWith("t1", "Narration", false);
  });

  it("selects and moves free clips in time, and audio clips between tracks", async () => {
    const ov = overlay({ id: "ov1", timeline_start: 1000, source_end: 2000 });
    const ac = audioClip({ id: "ac1", timeline_start: 0, source_end: 1000 });
    const p = project([clip({ id: "a" })], { overlays: [ov], audio_tracks: [audioTrack([ac], { id: "t1" }), audioTrack([], { id: "t2" })] });
    const { store, w } = setup(p);
    await w.vm.$nextTick();
    const scroller = w.find(".overflow-x-auto");
    const [ovBlock, acBlock] = w.findAllComponents(FreeBlock);
    await ovBlock.trigger("pointerdown", { clientX: 100, pointerId: 1 });
    expect(store.selected).toEqual({ kind: "overlay", id: "ov1" });
    await scroller.trigger("pointermove", { clientX: 100 + 2000 * PX_PER_MS });
    expect(w.findAllComponents(FreeBlock)[0].props("clip").timeline_start).toBeCloseTo(3000, 0);
    await scroller.trigger("pointerup");
    expect(api.overlayMove).toHaveBeenCalledWith("ov1", 3000, 0);
    // audio: dropped over the second track's row
    const t2row = w.find("[data-row='audio:t2']").element;
    const efp = vi.spyOn(document, "elementFromPoint").mockReturnValue(t2row);
    await acBlock.trigger("pointerdown", { clientX: 100, pointerId: 1 });
    expect(store.selected).toEqual({ kind: "audio", id: "ac1", trackId: "t1" });
    await scroller.trigger("pointermove", { clientX: 100 + 500 * PX_PER_MS, clientY: 300 });
    await scroller.trigger("pointerup");
    expect(api.audioClipMove).toHaveBeenCalledWith("ac1", "t2", 500);
    efp.mockRestore();
    // a tiny movement is a click, not a move
    await ovBlock.trigger("pointerdown", { clientX: 100, pointerId: 1 });
    await scroller.trigger("pointermove", { clientX: 102 });
    await scroller.trigger("pointerup");
    expect(api.overlayMove).toHaveBeenCalledTimes(1);
  });

  it("trims free clips; a left trim on video keeps the content anchored, stills only set a length", async () => {
    const ov = overlay({ id: "ov1", timeline_start: 1000, media: stillMedia() });
    const ac = audioClip({ id: "ac1", timeline_start: 2000, source_end: 3000, media: audioMedia({ duration_ms: 3000 }) });
    const { w } = setup(project([clip({ id: "a" })], { overlays: [ov], audio_tracks: [audioTrack([ac], { id: "t1" })] }));
    await w.vm.$nextTick();
    const scroller = w.find(".overflow-x-auto");
    const [ovBlock, acBlock] = w.findAllComponents(FreeBlock);
    // still: drag the right edge out past its 5 s default
    await ovBlock.findAll(".cursor-ew-resize")[1].trigger("pointerdown", { clientX: 100, pointerId: 1 });
    await scroller.trigger("pointermove", { clientX: 100 + 3000 * PX_PER_MS });
    await scroller.trigger("pointerup");
    await flush();
    expect(api.overlayTrim).toHaveBeenCalledWith("ov1", 0, 8000);
    expect(api.overlayMove).not.toHaveBeenCalled();
    // audio: drag the left edge in by 1 s → source_start 1000, and the clip moves right by 1 s
    await acBlock.findAll(".cursor-ew-resize")[0].trigger("pointerdown", { clientX: 100, pointerId: 1 });
    await scroller.trigger("pointermove", { clientX: 100 + 1000 * PX_PER_MS });
    await scroller.trigger("pointerup");
    await flush();
    expect(api.audioClipTrim).toHaveBeenCalledWith("ac1", 1000, 3000);
    expect(api.audioClipMove.mock.calls).toEqual([["ac1", "t1", 3000]]);
  });

  it("accepts pool drops on matching rows only", async () => {
    const p = project([clip({ id: "a" }), clip({ id: "b", source_end: 4000 })], { audio_tracks: [audioTrack([], { id: "t1" })] });
    const { w } = setup(p);
    await w.vm.$nextTick();
    const scroller = w.find(".overflow-x-auto").element as HTMLElement;
    vi.spyOn(scroller, "getBoundingClientRect").mockReturnValue({ left: 0, top: 0, width: 1024, height: 200, right: 1024, bottom: 200, x: 0, y: 0, toJSON: () => ({}) });
    const drop = (row: string, item: ReturnType<typeof poolItem>, clientX: number) =>
      w.find(`[data-row='${row}']`).element.dispatchEvent(new CustomEvent("pooldrop", { detail: { item, clientX } }));
    const png = poolItem({ media: stillMedia(), path: "/images/logo.png" });
    const wav = poolItem({ media: audioMedia(), path: "/audio/x.wav" });
    const mp4 = poolItem({ path: "/videos/c.mp4" });
    drop("overlay:0", png, 24 + 1500 * PX_PER_MS);
    drop("overlay:0", wav, 24);
    drop("video", wav, 24);
    drop("video", png, 24);
    drop("video", mp4, 24 + 7000 * PX_PER_MS);
    drop("audio:t1", wav, 24 + 250 * PX_PER_MS);
    drop("audio:t1", png, 24);
    await flush();
    expect(api.overlayAdd).toHaveBeenCalledTimes(1);
    expect(api.overlayAdd).toHaveBeenCalledWith("/images/logo.png", 1500, 0);
    expect(api.clipInsert.mock.calls).toEqual([["/images/logo.png", 0], ["/videos/c.mp4", 1]]);
    expect(api.audioClipAdd).toHaveBeenCalledTimes(1);
    expect(api.audioClipAdd).toHaveBeenCalledWith("t1", "/audio/x.wav", 250);
  });

  it("overlay layers: rows per layer, add / remove, drag between layers, drop onto a higher layer", async () => {
    const lo = overlay({ id: "lo", timeline_start: 0, source_end: 2000, layer: 0 });
    const hi = overlay({ id: "hi", timeline_start: 500, source_end: 2000, layer: 1 });
    const { w } = setup(project([clip({ id: "a" })], { overlays: [lo, hi], overlay_layers: 2 }));
    await w.vm.$nextTick();
    expect(w.text()).toContain("V3 · overlay");
    const rows = w.findAll("[data-row^='overlay:']");
    expect(rows.map((r) => r.attributes("data-row"))).toEqual(["overlay:0", "overlay:1"]);
    // V3 is drawn above V2
    expect(parseFloat((rows[1].element as HTMLElement).style.top)).toBeLessThan(parseFloat((rows[0].element as HTMLElement).style.top));
    expect(rows[1].findAllComponents(FreeBlock)[0].props("clip").id).toBe("hi");
    await w.findAll("button").find((b) => b.text() === "+ Layer")!.trigger("click");
    expect(api.overlayLayerAdd).toHaveBeenCalled();
    await w.find("button[title='Remove layer V3']").trigger("click");
    expect(api.overlayLayerRemove).toHaveBeenCalledWith(1);
    // drag lo up onto the V3 row
    const scroller = w.find(".overflow-x-auto");
    const efp = vi.spyOn(document, "elementFromPoint").mockReturnValue(rows[1].element);
    await rows[0].findComponent(FreeBlock).trigger("pointerdown", { clientX: 100, pointerId: 1 });
    await scroller.trigger("pointermove", { clientX: 100 + 3000 * PX_PER_MS, clientY: 40 });
    await scroller.trigger("pointerup");
    expect(api.overlayMove).toHaveBeenCalledWith("lo", 3000, 1);
    efp.mockRestore();
    const scrollerEl = scroller.element as HTMLElement;
    vi.spyOn(scrollerEl, "getBoundingClientRect").mockReturnValue({ left: 0, top: 0, width: 1024, height: 200, right: 1024, bottom: 200, x: 0, y: 0, toJSON: () => ({}) });
    rows[1].element.dispatchEvent(new CustomEvent("pooldrop", { detail: { item: poolItem({ path: "/videos/c.mp4" }), clientX: 24 + 1000 * PX_PER_MS } }));
    await flush();
    expect(api.overlayAdd).toHaveBeenCalledWith("/videos/c.mp4", 1000, 1);
  });
});
