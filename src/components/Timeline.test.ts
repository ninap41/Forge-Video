import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { clip, music, project, resolveWith, type MockApi } from "../test/fixtures";

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

  it("shows the music block when a bed is set", async () => {
    const { w } = setup(project([clip()], { music: music({ trim_start: 0, trim_end: 3000, timeline_start: 1000 }) }));
    await w.vm.$nextTick();
    expect(w.text()).toContain("♪ music");
    expect(w.text()).not.toContain("No music");
    const { w: none } = setup();
    expect(none.text()).toContain("No music");
  });
});
