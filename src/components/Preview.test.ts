import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { audioClip, audioTrack, clip, cue, media, overlay, project, resolveWith, stillMedia, textClip, type MockApi } from "../test/fixtures";

vi.mock("../api/tauri", async () => {
  const f = await import("../test/fixtures");
  return { api: f.mockApi(f.project([])) };
});
import { api as apiModule } from "../api/tauri";
const api = apiModule as unknown as MockApi;

import Preview from "./Preview.vue";
import { useProjectStore } from "../stores/project";
import { FAKE_PNG_URL, resizeAll } from "../test/setup";

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
    expect(w.text()).toContain("Drop a video or image here");
    expect(w.text()).toContain("Trim · Split · Fade · Dissolve · Overlays · Audio tracks · Aspect · Export");
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
    // zoom out to 0.5×: the source is half the window's size, centred on the frame
    store.project!.crop = { scale: 0.5, x: 0.5, y: 0.5 };
    await w.vm.$nextTick();
    const k2 = (253.125 / 607.5) * 0.5;
    expect(px(v().width)).toBeCloseTo(1920 * k2, 2);
    expect(px(v().left)).toBeCloseTo(253.125 / 2 - 960 * k2, 2);
    expect(px(v().top)).toBeCloseTo(450 / 2 - 540 * k2, 2);
    // and with x = 0 it is not clamped: its left edge sits at the frame centre, half of it off-frame
    store.project!.crop = { scale: 0.5, x: 0, y: 0.5 };
    await w.vm.$nextTick();
    expect(px(v().left)).toBeCloseTo(253.125 / 2, 2);
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
    // zooming out goes below 1 and stops at 0.25
    for (let i = 0; i < 60; i++) await frame.trigger("wheel", { deltaY: 100 });
    vi.advanceTimersByTime(250);
    expect(api.setCrop).toHaveBeenLastCalledWith(expect.objectContaining({ scale: 0.25 }));
    vi.useRealTimers();
  });

  it("play/pause drives the media elements and rewinds from the end", async () => {
    const ac = audioClip({ id: "ac1", source: "/audio/bed.m4a", source_end: 20000 });
    const { store, w } = await setup(project([clip({ id: "a" })], { audio_tracks: [audioTrack([ac])] }));
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

  it("mounts one <audio> per active audio clip, muting silent ones", async () => {
    const a1 = audioClip({ id: "a1", source: "/audio/one.m4a", timeline_start: 0, source_end: 3000, volume: 0.4 });
    const a2 = audioClip({ id: "a2", source: "/audio/two.m4a", timeline_start: 2000, source_end: 2000 });
    const { store, w } = await setup(project([clip({ id: "a" })], { audio_tracks: [audioTrack([a1], { id: "t1" }), audioTrack([a2], { id: "t2", muted: true })] }));
    store.playhead = 1000;
    await flush();
    expect(w.findAll("[data-testid=audio-clip]")).toHaveLength(1);
    store.playhead = 2500;
    await flush();
    const els = w.findAll("[data-testid=audio-clip]").map((e) => e.element as HTMLAudioElement);
    expect(els.map((e) => e.getAttribute("src"))).toEqual(["asset://localhost/audio/one.m4a", "asset://localhost/audio/two.m4a"]);
    expect(els[0].volume).toBeCloseTo(0.4);
    expect(els[1].volume).toBe(0);
    // the track fader scales the clip's own volume
    store.project = { ...store.project!, audio_tracks: [audioTrack([a1], { id: "t1", volume: 0.5 }), audioTrack([a2], { id: "t2", muted: true })] };
    await flush();
    expect((w.findAll("[data-testid=audio-clip]")[0].element as HTMLAudioElement).volume).toBeCloseTo(0.2);
    store.playhead = 4500;
    await flush();
    expect(w.findAll("[data-testid=audio-clip]")).toHaveLength(0);
  });

  it("layers the current overlay with its placement and fades, and drags it when selected", async () => {
    vi.useFakeTimers();
    const ov = overlay({ id: "ov1", media: stillMedia(), timeline_start: 1000, source_end: 2000, fade_in: 500, placement: { scale: 0.5, x: 0.5, y: 0.5 } });
    const { store, w } = await setup(project([clip({ id: "a" })], { overlays: [ov] }));
    store.playhead = 0;
    await w.vm.$nextTick();
    expect(w.find("[data-testid=overlay-layer]").exists()).toBe(false);
    store.playhead = 1250;
    await w.vm.$nextTick();
    const layer = w.find("[data-testid=overlay-layer]");
    expect(layer.element.tagName).toBe("IMG");
    // frame is 800×450; overlay half the width, 400×300 source → 400×300 px, centred
    expect(px(layer.attributes("style")!.match(/width: ([\d.]+)px/)![1])).toBeCloseTo(400, 0);
    expect(px(layer.attributes("style")!.match(/left: ([\d.]+)px/)![1])).toBeCloseTo(200, 0);
    expect(layer.attributes("style")).toContain("opacity: 0.5");
    // constant opacity multiplies the fade
    store.project = { ...store.project!, overlays: [{ ...ov, opacity: 0.5 }] };
    await w.vm.$nextTick();
    expect(w.find("[data-testid=overlay-layer]").attributes("style")).toContain("opacity: 0.25");
    // not selected: dragging the frame moves the crop, not the overlay
    const frame = w.find(".cursor-grab");
    await frame.trigger("pointerdown", { clientX: 100, clientY: 100, pointerId: 1 });
    await frame.trigger("pointermove", { clientX: 140, clientY: 100 });
    await frame.trigger("pointerup");
    expect(api.setCrop).toHaveBeenCalledTimes(1);
    expect(api.overlaySetPlacement).not.toHaveBeenCalled();
    // selected: the same drag places the overlay (80 px right of an 800 px frame = +0.1)
    store.select({ kind: "overlay", id: "ov1" });
    await w.vm.$nextTick();
    await frame.trigger("pointerdown", { clientX: 100, clientY: 100, pointerId: 1 });
    await frame.trigger("pointermove", { clientX: 180, clientY: 145 });
    await frame.trigger("pointerup");
    expect(api.overlaySetPlacement).toHaveBeenCalledWith("ov1", { scale: 0.5, x: expect.closeTo(0.6, 3), y: expect.closeTo(0.6, 3) });
    expect(api.setCrop).toHaveBeenCalledTimes(1);
    await frame.trigger("wheel", { deltaY: -100 });
    vi.advanceTimersByTime(250);
    expect(api.overlaySetPlacement).toHaveBeenLastCalledWith("ov1", expect.objectContaining({ scale: expect.closeTo(0.525, 3) }));
    vi.useRealTimers();
  });

  it("draws the current title from the canvas raster, moves it by drag and resizes it with the wheel when selected", async () => {
    vi.useFakeTimers();
    const t = textClip({ id: "t1", timeline_start: 1000, duration: 2000, fade_in: 500, x: 0.5, y: 0.5, style: { text: "Hey", font: "Impact", size: 0.1, color: "#ffffff", backdrop: null } });
    const { store, w } = await setup(project([clip({ id: "a" })], { texts: [t] }));
    store.playhead = 0;
    await w.vm.$nextTick();
    expect(w.find("[data-testid=text-layer]").exists()).toBe(false);
    store.playhead = 1250;
    await w.vm.$nextTick();
    const layer = w.find("[data-testid=text-layer]");
    expect(layer.attributes("src")).toBe(FAKE_PNG_URL);
    // frame 800×450: 10 % → 45 px font, pad 20.25, 3 chars × 10 px → box 71 × 95 px, centred
    const style = layer.attributes("style")!;
    expect(px(style.match(/width: ([\d.]+)px/)![1])).toBe(71);
    expect(px(style.match(/height: ([\d.]+)px/)![1])).toBe(95);
    expect(px(style.match(/left: ([\d.]+)px/)![1])).toBeCloseTo(400 - 35.5, 1);
    expect(style).toContain("opacity: 0.5");
    const frame = w.find(".cursor-grab");
    await frame.trigger("pointerdown", { clientX: 100, clientY: 100, pointerId: 1 });
    await frame.trigger("pointermove", { clientX: 140, clientY: 100 });
    await frame.trigger("pointerup");
    expect(api.setCrop).toHaveBeenCalledTimes(1);
    expect(api.textSetPosition).not.toHaveBeenCalled();
    store.select({ kind: "text", id: "t1" });
    await w.vm.$nextTick();
    expect(w.text()).toContain("drag to place title");
    await frame.trigger("pointerdown", { clientX: 100, clientY: 100, pointerId: 1 });
    await frame.trigger("pointermove", { clientX: 180, clientY: 145 });
    await frame.trigger("pointerup");
    expect(api.textSetPosition).toHaveBeenCalledWith("t1", expect.closeTo(0.6, 3), expect.closeTo(0.6, 3));
    expect(api.setCrop).toHaveBeenCalledTimes(1);
    await frame.trigger("wheel", { deltaY: -100 });
    vi.advanceTimersByTime(250);
    expect(api.textSetStyle).toHaveBeenCalledWith("t1", expect.objectContaining({ font: "Impact", size: expect.closeTo(0.105, 3) }));
    vi.useRealTimers();
  });

  it("stacks every overlay under the playhead in layer order", async () => {
    const lo = overlay({ id: "lo", media: stillMedia(), timeline_start: 0, source_end: 3000, layer: 0 });
    const hi = overlay({ id: "hi", media: stillMedia(), timeline_start: 1000, source_end: 3000, layer: 1, placement: { scale: 0.2, x: 0.1, y: 0.1 } });
    const { store, w } = await setup(project([clip({ id: "a" })], { overlays: [lo, hi], overlay_layers: 2 }));
    store.playhead = 500;
    await w.vm.$nextTick();
    expect(w.findAll("[data-testid=overlay-layer]")).toHaveLength(1);
    store.playhead = 1500;
    await w.vm.$nextTick();
    const layers = w.findAll("[data-testid=overlay-layer]");
    expect(layers).toHaveLength(2);
    expect(px(layers[1].attributes("style")!.match(/width: ([\d.]+)px/)![1])).toBeCloseTo(160, 0);
  });

  it("plays a video overlay's sound at clip × row volume, silent when either is muted", async () => {
    const ov = overlay({ id: "ov", media: media({ has_audio: true }), timeline_start: 0, source_end: 3000, volume: 0.8 });
    const { store, w } = await setup(project([clip({ id: "a" })], { overlays: [ov], overlay_audio: [{ muted: false, volume: 0.5 }] }));
    store.playhead = 1000;
    await flush();
    const el = w.find("[data-testid=overlay-layer]").element as HTMLVideoElement;
    expect(el.tagName).toBe("VIDEO");
    expect(el.muted).toBe(false);
    expect(el.volume).toBeCloseTo(0.4);
    store.project = { ...store.project!, overlay_audio: [{ muted: true, volume: 0.5 }] };
    await flush();
    expect(el.muted).toBe(true);
    store.project = { ...store.project!, overlay_audio: [{ muted: false, volume: 1 }], overlays: [{ ...ov, muted: true }] };
    await flush();
    expect(el.muted).toBe(true);
    store.project = { ...store.project!, overlays: [{ ...ov, muted: false, volume: 1.5 }] };
    await flush();
    expect(el.muted).toBe(false);
    expect(el.volume).toBe(1); // clip volumes above 100 % are capped in the preview
  });

  it("an unplayable source raises a banner once and does not stall the clip change", async () => {
    const { store, w } = await setup(project([clip({ id: "mkv", source: "/v/a.mkv" }), clip({ id: "b", source: "/v/b.mp4" })]));
    await w.vm.$nextTick();
    const v = w.find("video").element as HTMLVideoElement;
    v.dispatchEvent(new Event("error"));
    expect(store.notice).toBe("Preview can't play a.mkv here — export still works");
    store.notify(null);
    v.dispatchEvent(new Event("error"));
    expect(store.notice).toBeNull();
    // with readyState 0 and no metadata, nextSrcReady must still resolve on error
    Object.defineProperty(HTMLMediaElement.prototype, "readyState", { value: 0, configurable: true });
    store.playhead = 6000;
    await w.vm.$nextTick();
    await flush();
    w.find("video").element.dispatchEvent(new Event("error"));
    await flush();
    expect(store.notice).toBe("Preview can't play b.mp4 here — export still works");
    Object.defineProperty(HTMLMediaElement.prototype, "readyState", { value: 1, configurable: true });
    // audio clips report too
    const wav = audioClip({ id: "ac", source: "/audio/x.ogg", timeline_start: 0 });
    store.project = project([clip({ id: "a", source: "/v/a.mp4" })], { audio_tracks: [audioTrack([wav], { id: "t1" })] });
    store.playhead = 100;
    await w.vm.$nextTick();
    store.notify(null);
    w.find("[data-testid=audio-clip]").element.dispatchEvent(new Event("error"));
    expect(store.notice).toBe("Preview can't play x.ogg here — export still works");
  });

  it("plays a still on V1 with a wall-clock, no <video>, then moves to the next clip", async () => {
    vi.useFakeTimers();
    const card = clip({ id: "card", source: "/images/title.png", media: stillMedia(), source_end: 1000 });
    const { store, w } = await setup(project([card, clip({ id: "b", source: "/v/b.mp4" })]));
    store.playhead = 0;
    await w.vm.$nextTick();
    expect(w.find("[data-testid=still-layer]").attributes("src")).toBe("asset://localhost/images/title.png");
    expect(w.find("video").exists()).toBe(false);
    store.playing = true;
    await vi.advanceTimersByTimeAsync(400);
    expect(store.playhead).toBeGreaterThan(300);
    expect(store.playhead).toBeLessThan(1000);
    await vi.advanceTimersByTimeAsync(800);
    expect(store.current?.clip.id).toBe("b");
    expect(w.find("video").exists()).toBe(true);
    store.playing = false;
    vi.useRealTimers();
  });

  it("looping a range wraps the playhead back to its start instead of moving on", async () => {
    vi.useFakeTimers();
    const card = clip({ id: "card", source: "/images/title.png", media: stillMedia(), source_end: 3000 });
    const { store, w } = await setup(project([card, clip({ id: "b", source: "/v/b.mp4" })]));
    store.setRange({ start: 500, end: 1000 });
    store.loopOn = true;
    store.playhead = 0;
    await w.vm.$nextTick();
    store.playing = true; // outside the range: playback starts at the range start
    await vi.advanceTimersByTimeAsync(0);
    expect(store.playhead).toBe(500);
    await vi.advanceTimersByTimeAsync(1200);
    expect(store.playing).toBe(true);
    expect(store.playhead).toBeGreaterThanOrEqual(500);
    expect(store.playhead).toBeLessThan(1000);
    expect(store.current?.clip.id).toBe("card");
    // loop off: the still runs to its end and hands over to the next clip
    store.loopOn = false;
    await vi.advanceTimersByTimeAsync(3500);
    expect(store.current?.clip.id).toBe("b");
    store.playing = false;
    vi.useRealTimers();
  });

  it("a video clip ending inside a loop at the end of the timeline wraps instead of stopping", async () => {
    const { store, w } = await setup(project([clip({ id: "a" })])); // 5 s
    await w.vm.$nextTick();
    const video = w.find("video").element as HTMLVideoElement;
    store.setRange({ start: 2000, end: 5000 });
    store.loopOn = true;
    store.playhead = 4000;
    store.playing = true;
    await flush();
    Object.defineProperty(video, "currentTime", { value: 4.995, writable: true, configurable: true });
    await flush();
    expect(store.playing).toBe(true);
    expect(store.playhead).toBe(2000);
    store.playing = false;
  });

  it("a muted video track silences the <video> even when the clip itself is not muted", async () => {
    const { store, w } = await setup(project([clip({ id: "a", volume: 0.8 })]));
    const video = w.find("video").element as HTMLVideoElement;
    store.playhead = 100;
    await flush();
    expect(video.volume).toBeCloseTo(0.8);
    store.project = { ...store.project!, video_muted: true };
    await flush();
    expect(video.volume).toBe(0);
    store.project = { ...store.project!, video_muted: false, video_volume: 0.5 };
    await flush();
    expect(video.volume).toBeCloseTo(0.4, 5);
  });

  it("shows the caption under the playhead, sized from the frame", async () => {
    const p = project([clip({ id: "a", source: "/v/a.mp4" }), clip({ id: "b", source: "/v/b.mp4", source_end: 4000 })], {
      transcripts: [{ source: "/v/b.mp4", cues: [cue(1000, 3000, "nobody tells you this")] }],
    });
    const { store, w } = await setup(p);
    expect(w.find("[data-testid=caption]").exists()).toBe(false);
    store.playhead = 5000 + 1500;
    await w.vm.$nextTick();
    const c = w.find("[data-testid=caption]");
    expect(c.text()).toBe("nobody tells you this");
    expect(parseFloat((c.element as HTMLElement).style.fontSize)).toBeCloseTo(450 * 0.045, 3);
    expect(c.classes()).toContain("pointer-events-none");
    store.playhead = 5000 + 3000;
    await w.vm.$nextTick();
    expect(w.find("[data-testid=caption]").exists()).toBe(false);
    // the captions track switched off hides it
    store.playhead = 5000 + 1500;
    await w.vm.$nextTick();
    expect(w.find("[data-testid=caption]").exists()).toBe(true);
    store.project = { ...store.project!, captions_enabled: false };
    await w.vm.$nextTick();
    expect(w.find("[data-testid=caption]").exists()).toBe(false);
  });
});
