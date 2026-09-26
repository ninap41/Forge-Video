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

import Preview from "./Preview.vue";
import { useProjectStore } from "../stores/project";
import { resizeAll } from "../test/setup";

const flush = () => new Promise((r) => setTimeout(r, 0));

async function setup(p = project([clip({ id: "a", source: "/v/a.mp4" }), clip({ id: "b", source: "/v/b.mp4", source_end: 4000 })])) {
  setActivePinia(createPinia());
  const store = useProjectStore();
  store.project = p;
  resolveWith(api, p);
  const w = mount(Preview, { attachTo: document.body });
  mounted.push(w);
  await w.vm.$nextTick(); // the stage ref is observed in a watcher
  resizeAll(800, 450);
  await w.vm.$nextTick();
  return { store, w };
}
const mounted: ReturnType<typeof mount>[] = [];
afterEach(() => { vi.useRealTimers(); mounted.splice(0).forEach((w) => w.unmount()); });
const px = (s: string) => parseFloat(s);

beforeEach(() => { for (const v of Object.values(api)) if (typeof v === "function" && "mockClear" in v) (v as ReturnType<typeof vi.fn>).mockClear(); });

describe("Preview", () => {
  it("shows the empty state with the feature list when there is no clip", async () => {
    const { w } = await setup(project([]));
    expect(w.text()).toContain("Drop a video here");
    expect(w.text()).toContain("Trim · Split · Fade · Dissolve · Music · Aspect · Export");
    expect(w.find("video").exists()).toBe(false);
  });

  it("letterboxes the output frame inside the stage for each preset", async () => {
    const { store, w } = await setup();
    await w.vm.$nextTick();
    const frame = () => (w.find(".cursor-grab").element as HTMLElement).style;
    expect(px(frame().width)).toBeCloseTo(800, 3);
    expect(px(frame().height)).toBeCloseTo(450, 3);
    store.project!.aspect = "Shorts9x16";
    await w.vm.$nextTick();
    expect(px(frame().height)).toBeCloseTo(450, 3);
    expect(px(frame().width)).toBeCloseTo(253.125, 3);
    expect(w.text()).toContain("Shorts / Reels 9:16");
    store.project!.aspect = "Square1x1";
    await w.vm.$nextTick();
    expect(px(frame().width)).toBeCloseTo(450, 3);
  });

  it("video element points at the asset URL of the clip under the playhead", async () => {
    const { store, w } = await setup();
    await w.vm.$nextTick();
    expect(w.find("video").attributes("src")).toBe("asset://localhost/v/a.mp4");
    store.playhead = 6000;
    await w.vm.$nextTick();
    expect(w.find("video").attributes("src")).toBe("asset://localhost/v/b.mp4");
  });

  it("positions the source to mirror render::graph::crop_scale_filter", async () => {
    const { store, w } = await setup();
    store.project!.aspect = "Shorts9x16";
    await w.vm.$nextTick();
    const v = () => (w.find("video").element as HTMLElement).style;
    // frame 253.125 wide; 9:16 window in 1920x1080 is 607.5 wide → k = 0.41667
    expect(px(v().width)).toBeCloseTo(800, 2);
    expect(px(v().height)).toBeCloseTo(450, 2);
    expect(px(v().left)).toBeCloseTo(-(960 - 303.75) * (253.125 / 607.5), 2);
    expect(px(v().top)).toBeCloseTo(0, 2);
    // zoom 2× pinned to the left edge (crop x clamped) → left 0, top = -(540-135)*k
    store.project!.crop = { scale: 2, x: 0, y: 0.5 };
    await w.vm.$nextTick();
    const k = 253.125 / 303.75;
    expect(px(v().left)).toBeCloseTo(0, 2);
    expect(px(v().top)).toBeCloseTo(-(540 - 270) * k, 2);
    expect(px(v().width)).toBeCloseTo(1920 * k, 2);
  });

  it("uses the rotated display size for portrait phone footage", async () => {
    const { w } = await setup(project([clip({ media: media({ rotation: 90 }) })], { aspect: "Shorts9x16" }));
    await w.vm.$nextTick();
    const v = (w.find("video").element as HTMLElement).style;
    // displayed 1080x1920 fills the 9:16 frame exactly
    expect(px(v.width)).toBeCloseTo(253.125, 2);
    expect(px(v.height)).toBeCloseTo(450, 2);
    expect(px(v.left)).toBeCloseTo(0, 2);
  });

  it("fades opacity for fade in/out and across transitions", async () => {
    const a = clip({ id: "a", fade_in: 1000, fade_out: 1000, transition_out: { type: "CrossDissolve", ms: 1000 } });
    const b = clip({ id: "b", source_end: 4000 });
    const { store, w } = await setup(project([a, b]));
    const op = () => parseFloat((w.find("video").element as HTMLElement).style.opacity);
    const at = async (t: number) => { store.playhead = t; await w.vm.$nextTick(); return op(); };
    expect(await at(0)).toBe(0);
    expect(await at(500)).toBeCloseTo(0.5, 5);
    expect(await at(2500)).toBe(1);
    expect(await at(4500)).toBeCloseTo(0.5, 5); // inside a's fade out AND its dissolve region → but b is current
    // at 4500 the *incoming* clip b is current (timeline_start 4000) and fades in across the dissolve
    expect(store.current?.clip.id).toBe("b");
    expect(await at(4000)).toBe(0);
    expect(await at(5000)).toBe(1);
  });

  it("dragging repositions the crop and commits on release; wheel zooms (debounced)", async () => {
    vi.useFakeTimers();
    const { w } = await setup();
    await w.vm.$nextTick();
    const frame = w.find(".cursor-grab");
    await frame.trigger("pointerdown", { clientX: 100, clientY: 100, pointerId: 1 });
    await frame.trigger("pointermove", { clientX: 60, clientY: 100 });
    await frame.trigger("pointerup");
    // k = 800/1920; dx = -40 px → +40/(1920*k) = +0.05
    expect(api.setCrop).toHaveBeenCalledWith({ scale: 1, x: expect.closeTo(0.55, 5), y: 0.5 });
    await frame.trigger("wheel", { deltaY: -100 });
    await frame.trigger("wheel", { deltaY: -100 });
    expect(api.setCrop).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(250);
    expect(api.setCrop).toHaveBeenCalledTimes(2);
    expect(api.setCrop).toHaveBeenLastCalledWith(expect.objectContaining({ scale: expect.closeTo(1.1025, 4) }));
    // zoom never drops below 1
    for (let i = 0; i < 10; i++) await frame.trigger("wheel", { deltaY: 100 });
    vi.advanceTimersByTime(250);
    expect(api.setCrop).toHaveBeenLastCalledWith(expect.objectContaining({ scale: 1 }));
    vi.useRealTimers();
  });

  it("play/pause drives the media elements and rewinds from the end", async () => {
    const { store, w } = await setup(project([clip({ id: "a" })], { music: music() }));
    await w.vm.$nextTick();
    const video = w.find("video").element as HTMLVideoElement;
    const audio = w.find("audio").element as HTMLAudioElement;
    expect(audio.getAttribute("src")).toBe("asset://localhost/audio/bed.m4a");
    store.playhead = 4999;
    store.playing = true;
    await flush();
    expect(store.playhead).toBe(0);
    expect(video.play).toHaveBeenCalled();
    store.playing = false;
    await w.vm.$nextTick();
    expect(video.pause).toHaveBeenCalled();
    expect(audio.pause).toHaveBeenCalled();
  });

  it("applies clip volume and mute to the video element when seeking", async () => {
    const { store, w } = await setup(project([clip({ id: "a", volume: 0.3 }), clip({ id: "b", muted: true })]));
    await w.vm.$nextTick();
    const video = w.find("video").element as HTMLVideoElement;
    store.playhead = 100;
    await flush();
    expect(video.volume).toBeCloseTo(0.3, 5);
    store.playhead = 6000;
    await flush();
    expect(video.volume).toBe(0);
  });
});
