import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { audioClip, audioTrack, clip, cue, highlight, media, overlay, poolItem, project, resolveWith, stillMedia, audioMedia, type MockApi } from "../test/fixtures";
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
    expect(w.text()).toContain("V2 · video");
    expect(w.text()).not.toContain("V3 · video");
    expect(w.text()).not.toContain("No audio tracks");
    const { w: none } = setup();
    expect(none.text()).toContain("No audio tracks");
    expect(none.text()).toContain("Drop video or a PNG here");
  });

  it("the V1 gutter has a track-level mute that also hides the waveforms", async () => {
    const { store, w } = setup();
    await w.vm.$nextTick();
    const mute = w.find("button[aria-label='Mute video audio']");
    expect(mute.classes()).toContain("text-accent");
    expect(w.findAllComponents(ClipBlock)[0].props("peaks")).toBeUndefined(); // no waveform cached yet in this test
    await mute.trigger("click");
    expect(api.setVideoMuted).toHaveBeenCalledWith(true);
    const fader = w.find("[data-testid=fader-v1]");
    expect(fader.attributes("title")).toBe("V1 volume 100 %");
    await fader.setValue("0.5");
    expect(api.setVideoVolume).toHaveBeenCalledWith(0.5);
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
    expect(gutterButtons.filter((b) => b.text().startsWith("+"))).toHaveLength(1);
    await w.find("[data-testid=add-track]").trigger("click");
    const items = w.findAll("[data-testid=add-track-menu] button");
    expect(items.map((b) => b.text())).toEqual(["▶ Video track", "♪ Audio track", "♪ Music track", "♪ Narration track", "♪ SFX track"]);
    await items[3].trigger("click");
    expect(api.audioTrackAdd).toHaveBeenCalledWith("Narration");
    expect(w.find("[data-testid=add-track-menu]").exists()).toBe(false);
    // the button toggles; Escape and outside clicks close
    await w.find("[data-testid=add-track]").trigger("click");
    await w.find("[data-testid=add-track]").trigger("click");
    expect(w.find("[data-testid=add-track-menu]").exists()).toBe(false);
    await w.find("[data-testid=add-track]").trigger("click");
    document.body.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
    await w.vm.$nextTick();
    expect(w.find("[data-testid=add-track-menu]").exists()).toBe(false);
    const mute = w.find("button[aria-label='Mute track']");
    expect(mute.classes()).toContain("text-accent");
    expect(mute.attributes("data-muted")).toBe("false");
    await mute.trigger("click");
    expect(api.audioTrackUpdate).toHaveBeenCalledWith("t1", "Music", true, 1);
    await w.find("button[title='Remove track']").trigger("click");
    expect(api.audioTrackRemove).toHaveBeenCalledWith("t1");
    await w.find("button[title='Rename Music']").trigger("click");
    const input = w.find("input[list=track-presets]");
    await input.setValue("Narration");
    await input.trigger("change");
    expect(api.audioTrackUpdate).toHaveBeenLastCalledWith("t1", "Narration", false, 1);
    // the track fader sends label + mute unchanged with the new level
    const fader = w.find("[data-testid=fader-t1]");
    expect(fader.attributes("title")).toBe("Music volume 100 %");
    await fader.setValue("0.25");
    expect(api.audioTrackUpdate).toHaveBeenLastCalledWith("t1", "Music", false, 0.25);
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

  it("accepts pool drops on matching rows only, and explains rejected drops in a banner", async () => {
    const p = project([clip({ id: "a" }), clip({ id: "b", source_end: 4000 })], { audio_tracks: [audioTrack([], { id: "t1" })] });
    const { store, w } = setup(p);
    expect(w.find("[data-testid=timeline-tracks]").exists()).toBe(true);
    await w.vm.$nextTick();
    const scroller = w.find(".overflow-x-auto").element as HTMLElement;
    vi.spyOn(scroller, "getBoundingClientRect").mockReturnValue({ left: 0, top: 0, width: 1024, height: 200, right: 1024, bottom: 200, x: 0, y: 0, toJSON: () => ({}) });
    const drop = (row: string, item: ReturnType<typeof poolItem>, clientX: number) =>
      w.find(`[data-row='${row}']`).element.dispatchEvent(new CustomEvent("pooldrop", { detail: { item, clientX } }));
    const png = poolItem({ media: stillMedia(), path: "/images/logo.png" });
    const wav = poolItem({ media: audioMedia(), path: "/audio/x.wav" });
    const mp4 = poolItem({ path: "/videos/c.mp4" });
    drop("overlay:0", png, 24 + 1500 * PX_PER_MS);
    expect(store.notice).toBeNull();
    drop("overlay:0", wav, 24);
    expect(store.notice).toBe("Audio goes on an audio track — drop it on a ♪ row");
    store.notify(null);
    drop("video", wav, 24);
    expect(store.notice).toBe("Audio goes on an audio track — drop it on a ♪ row");
    store.notify(null);
    drop("video", png, 24);
    drop("video", mp4, 24 + 7000 * PX_PER_MS);
    drop("audio:t1", wav, 24 + 250 * PX_PER_MS);
    expect(store.notice).toBeNull();
    drop("audio:t1", png, 24);
    expect(store.notice).toBe("logo.png has no audio — video and images go on V1 or a video track");
    store.notify(null);
    drop("audio:t1", mp4, 24);
    expect(store.notice).toBeNull();
    await flush();
    drop("audio:t1", poolItem({ path: "/videos/silent.mp4", media: media({ has_audio: false }) }), 24);
    expect(store.notice).toBe("silent.mp4 has no audio — video and images go on V1 or a video track");
    store.notify(null);
    expect(api.overlayAdd).toHaveBeenCalledTimes(1);
    expect(api.overlayAdd).toHaveBeenCalledWith("/images/logo.png", 1500, 0);
    expect(api.clipInsert.mock.calls).toEqual([["/images/logo.png", 0], ["/videos/c.mp4", 1]]);
    expect(api.audioClipAdd).toHaveBeenCalledTimes(2);
    expect(api.audioClipAdd).toHaveBeenCalledWith("t1", "/audio/x.wav", 250);
    expect(api.audioClipAdd).toHaveBeenCalledWith("t1", "/videos/c.mp4", 0);
  });

  it("right-click on a V1 clip offers Split audio from video; disabled for stills; closes on Escape or outside click", async () => {
    const still = clip({ id: "png", source: "/images/logo.png", media: stillMedia(), source_end: 1000 });
    const { store, w } = setup(project([clip({ id: "a" }), still]));
    await w.vm.$nextTick();
    expect(w.find("[data-testid=clip-menu]").exists()).toBe(false);
    const blocks = w.findAllComponents(ClipBlock);
    blocks[0].element.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 120, clientY: 80 }));
    await w.vm.$nextTick();
    const menu = w.find("[data-testid=clip-menu]");
    expect(menu.exists()).toBe(true);
    expect((menu.element as HTMLElement).style.left).toBe("120px");
    expect(store.selectedClipId).toBe("a");
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    await w.vm.$nextTick();
    expect(w.find("[data-testid=clip-menu]").exists()).toBe(false);
    blocks[0].element.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 10, clientY: 10 }));
    await w.vm.$nextTick();
    await w.find("[data-testid=clip-menu] button").trigger("click");
    await flush();
    expect(api.clipDetachAudio).toHaveBeenCalledWith("a", null);
    expect(w.find("[data-testid=clip-menu]").exists()).toBe(false);
    // a still has nothing to split
    blocks[1].element.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 10, clientY: 10 }));
    await w.vm.$nextTick();
    const item = w.find("[data-testid=clip-menu] button");
    expect(item.attributes("disabled")).toBeDefined();
    expect(item.attributes("title")).toContain("no audio");
    document.body.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
    await w.vm.$nextTick();
    expect(w.find("[data-testid=clip-menu]").exists()).toBe(false);
    expect(api.clipDetachAudio).toHaveBeenCalledTimes(1);
  });

  it("⇧-click selects several pieces and right-click offers Merge", async () => {
    const { store, w } = setup(project([clip({ id: "a", source: "/v/x.mp4", source_end: 2000 }), clip({ id: "b", source: "/v/x.mp4", source_start: 2000, source_end: 4000 }), clip({ id: "c" })]));
    await w.vm.$nextTick();
    const blocks = w.findAllComponents(ClipBlock);
    await blocks[0].trigger("pointerdown");
    await blocks[1].trigger("pointerdown", { shiftKey: true });
    expect(store.selectedAll.map((s) => s.id)).toEqual(["a", "b"]);
    expect(blocks[0].props("selected") && blocks[1].props("selected")).toBe(true);
    expect(blocks[2].props("selected")).toBe(false);
    // right-clicking a selected piece keeps the pair; the menu offers Merge
    blocks[1].element.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 10, clientY: 10 }));
    await w.vm.$nextTick();
    expect(store.selectedAll.map((s) => s.id)).toEqual(["a", "b"]);
    const merge = w.find("[data-testid=clip-merge]");
    expect(merge.text()).toBe("Merge 2 clips");
    await merge.trigger("click");
    await flush();
    expect(api.clipMerge).toHaveBeenCalledWith(["a", "b"]);
    expect(w.find("[data-testid=clip-menu]").exists()).toBe(false);
    // right-clicking an unselected clip collapses the selection: no Merge
    await blocks[0].trigger("pointerdown");
    await blocks[1].trigger("pointerdown", { shiftKey: true });
    blocks[2].element.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 10, clientY: 10 }));
    await w.vm.$nextTick();
    expect(store.selectedAll.map((s) => s.id)).toEqual(["c"]);
    expect(w.find("[data-testid=clip-merge]").exists()).toBe(false);
  });

  it("right-click → Rename… works on V1, overlay and audio clips", async () => {
    const ov = overlay({ id: "o", source_end: 2000 });
    const ac = audioClip({ id: "m", source: "/audio/bed.m4a", name: "Theme" });
    const { store, w } = setup(project([clip({ id: "a", source: "/videos/a.mp4" })], { overlays: [ov], audio_tracks: [audioTrack([ac], { id: "t1" })] }));
    await w.vm.$nextTick();
    expect(w.text()).toContain("♪ Theme");
    const rename = async (el: Element, value: string, key = "Enter") => {
      el.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 10, clientY: 10 }));
      await w.vm.$nextTick();
      await w.find("[data-testid=clip-rename]").trigger("click");
      const input = w.find("[data-testid=clip-name]");
      const was = (input.element as HTMLInputElement).value;
      (input.element as HTMLInputElement).value = value;
      await input.trigger("keydown", { key });
      await flush();
      expect(w.find("[data-testid=clip-menu]").exists()).toBe(false);
      return was;
    };
    expect(await rename(w.findComponent(ClipBlock).element, " Intro ")).toBe("a.mp4");
    expect(api.clipRename).toHaveBeenLastCalledWith("a", "Intro");
    const free = w.findAllComponents(FreeBlock);
    await rename(free[0].element, "Logo");
    expect(api.clipRename).toHaveBeenLastCalledWith("o", "Logo");
    expect(store.selected).toEqual({ kind: "overlay", id: "o" });
    // overlay and audio menus have no "Split audio from video"
    expect(await rename(free[1].element, "")).toBe("Theme");
    expect(api.clipRename).toHaveBeenLastCalledWith("m", "");
    expect(api.clipRename).toHaveBeenCalledTimes(3);
    // unchanged, the file name typed back on an unnamed clip, and Escape are all no-ops
    await rename(free[1].element, "Theme");
    await rename(w.findComponent(ClipBlock).element, "a.mp4");
    await rename(free[0].element, "Nope", "Escape");
    expect(api.clipRename).toHaveBeenCalledTimes(3);
  });

  it("video tracks above V1: rows per layer, add / remove, drag between layers, drop onto a higher layer", async () => {
    const lo = overlay({ id: "lo", timeline_start: 0, source_end: 2000, layer: 0 });
    const hi = overlay({ id: "hi", timeline_start: 500, source_end: 2000, layer: 1 });
    const { w } = setup(project([clip({ id: "a" })], { overlays: [lo, hi], overlay_layers: 2 }));
    await w.vm.$nextTick();
    expect(w.text()).toContain("V2 · video");
    expect(w.text()).toContain("V3 · video");
    const rows = w.findAll("[data-row^='overlay:']");
    expect(rows.map((r) => r.attributes("data-row"))).toEqual(["overlay:0", "overlay:1"]);
    // V3 is drawn above V2
    expect(parseFloat((rows[1].element as HTMLElement).style.top)).toBeLessThan(parseFloat((rows[0].element as HTMLElement).style.top));
    expect(rows[1].findAllComponents(FreeBlock)[0].props("clip").id).toBe("hi");
    await w.find("[data-testid=add-track]").trigger("click");
    await w.find("[data-testid=add-track-menu] button").trigger("click");
    expect(api.overlayLayerAdd).toHaveBeenCalled();
    await w.find("button[title='Remove track V3']").trigger("click");
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

  describe("captions and highlights", () => {
    const talk = () => project([clip({ id: "a", source: "/v/a.mp4" }), clip({ id: "b", source: "/v/b.mp4", source_end: 4000 })], {
      transcripts: [{ source: "/v/a.mp4", cues: [cue(0, 2000, "So here is the thing", { id: "c1" }), cue(2500, 4500, "nobody tells you", { id: "c2" })] }],
      audio_tracks: [audioTrack([audioClip()])],
    });
    const top = (w: ReturnType<typeof mount>, sel: string) => parseFloat((w.find(sel).element as HTMLElement).style.top);

    it("has no caption row until something is transcribed", () => {
      const { w } = setup(project([clip({ id: "a" })], { audio_tracks: [audioTrack([audioClip()])] }));
      expect(w.find("[data-row=caption]").exists()).toBe(false);
      expect(w.find("[data-testid=caption-gutter]").exists()).toBe(false);
      expect(top(w, "[data-row^='audio:']")).toBe(24 + 6 + 44 + 6 + 78 + 6);
    });

    it("lays captions out under V1 and moves the audio rows down", async () => {
      const { w } = setup(talk());
      await w.vm.$nextTick();
      expect(w.find("[data-testid=caption-gutter]").text()).toBe("CC · captions");
      expect(top(w, "[data-row=caption]")).toBe(24 + 6 + 44 + 6 + 78 + 6);
      expect(top(w, "[data-row^='audio:']")).toBe(24 + 6 + 44 + 6 + 78 + 6 + 22 + 6);
      const cues = w.findAll("[data-testid=cue]");
      expect(cues.map((c) => c.text())).toEqual(["So here is the thing", "nobody tells you"]);
      const st = (cues[1].element as HTMLElement).style;
      expect(parseFloat(st.left)).toBeCloseTo(2500 * PX_PER_MS, 3);
      expect(parseFloat(st.width)).toBeCloseTo(2000 * PX_PER_MS - 1, 3);
    });

    it("marks the caption under the playhead", async () => {
      const { store, w } = setup(talk());
      store.playhead = 3000;
      await w.vm.$nextTick();
      const cues = w.findAll("[data-testid=cue]");
      expect(cues[0].classes()).not.toContain("border-accent/70");
      expect(cues[1].classes()).toContain("border-accent/70");
    });

    it("double-click corrects a caption; Escape and unchanged text do nothing", async () => {
      const { w } = setup(talk());
      await w.vm.$nextTick();
      const first = () => w.findAll("[data-testid=cue]")[0];
      await first().trigger("dblclick");
      const input = w.find("[data-testid=cue-text]");
      expect((input.element as HTMLInputElement).value).toBe("So here is the thing");
      await input.setValue("  So here's the thing ");
      await input.trigger("keydown", { key: "Enter" });
      expect(api.cueSetText).toHaveBeenCalledWith("c1", "So here's the thing");
      expect(w.find("[data-testid=cue-text]").exists()).toBe(false);

      api.cueSetText.mockClear();
      await first().trigger("dblclick");
      await w.find("[data-testid=cue-text]").setValue("discarded");
      await w.find("[data-testid=cue-text]").trigger("keydown", { key: "Escape" });
      expect(w.find("[data-testid=cue-text]").exists()).toBe(false);
      await first().trigger("dblclick");
      await w.find("[data-testid=cue-text]").trigger("blur");
      expect(api.cueSetText).not.toHaveBeenCalled();
    });

    it("draws a band over every row for each highlight", async () => {
      const { w } = setup(project([clip({ id: "a" }), clip({ id: "b", source_end: 4000 })], { highlights: [highlight({ start: 1000, end: 6000 }), highlight({ start: 7000, end: 9000 })] }));
      await w.vm.$nextTick();
      const bands = w.findAll("[data-testid=highlight-band]");
      expect(bands).toHaveLength(2);
      const st = (bands[0].element as HTMLElement).style;
      expect(parseFloat(st.left)).toBeCloseTo(1000 * PX_PER_MS + 24, 3);
      expect(parseFloat(st.width)).toBeCloseTo(5000 * PX_PER_MS, 3);
      expect(st.top).toBe("24px");
      expect(bands[0].classes()).toContain("pointer-events-none");
    });
  });
});
