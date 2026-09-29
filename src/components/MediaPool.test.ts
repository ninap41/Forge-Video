import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { audioMedia, audioTrack, clip, poolItem, project, resolveWith, stillMedia, type MockApi } from "../test/fixtures";

vi.mock("../api/tauri", async () => {
  const f = await import("../test/fixtures");
  return { api: f.mockApi(f.project([])) };
});
import { api as apiModule } from "../api/tauri";
const api = apiModule as unknown as MockApi;
const dialogOpen = vi.fn();
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: (...a: unknown[]) => dialogOpen(...(a as [])) }));

import MediaPool from "./MediaPool.vue";
import { useProjectStore } from "../stores/project";

const items = () => [
  poolItem({ id: "v1", path: "/videos/a.mp4" }),
  poolItem({ id: "v2", path: "/videos/b.mov" }),
  poolItem({ id: "a1", path: "/audio/bed.m4a", media: audioMedia() }),
  poolItem({ id: "i1", path: "/images/logo.png", media: stillMedia() }),
];
function setup(p = project([clip({ id: "a" })], { pool: items() })) {
  setActivePinia(createPinia());
  const store = useProjectStore();
  store.project = p;
  resolveWith(api, p);
  const w = mount(MediaPool, { attachTo: document.body });
  mounted.push(w);
  return { store, w };
}
const mounted: ReturnType<typeof mount>[] = [];
afterEach(() => { mounted.splice(0).forEach((w) => w.unmount()); localStorage.clear(); });
beforeEach(() => { for (const v of Object.values(api)) if (typeof v === "function" && "mockClear" in v) (v as ReturnType<typeof vi.fn>).mockClear(); dialogOpen.mockReset(); });
const btn = (w: ReturnType<typeof mount>, t: string) => w.findAll("button").find((b) => b.text().includes(t))!;
const flush = () => new Promise((r) => setTimeout(r, 0));

describe("MediaPool", () => {
  it("tabs filter by kind and show counts; All is the default and lists everything", async () => {
    const { w } = setup();
    expect(w.text()).toContain("All 4");
    expect(w.text()).toContain("Clips 2");
    expect(w.text()).toContain("Audio 1");
    expect(w.text()).toContain("Images 1");
    for (const n of ["a.mp4", "b.mov", "bed.m4a", "logo.png"]) expect(w.text()).toContain(n);
    await btn(w, "Clips").trigger("click");
    expect(w.text()).toContain("a.mp4");
    expect(w.text()).not.toContain("bed.m4a");
    await btn(w, "Audio").trigger("click");
    expect(w.text()).toContain("bed.m4a");
    expect(w.text()).not.toContain("a.mp4");
    await btn(w, "Images").trigger("click");
    expect(w.text()).toContain("logo.png");
    expect(w.find("img").attributes("src")).toBe("asset://localhost/images/logo.png");
    const { w: empty } = setup(project([]));
    expect(empty.text()).toContain("Nothing here yet");
  });

  it("toggles grid / list and persists the choice", async () => {
    const { store, w } = setup();
    await btn(w, "Clips").trigger("click");
    expect(w.find("table").exists()).toBe(false);
    await w.find("button[aria-label='List view']").trigger("click");
    expect(store.poolView).toBe("list");
    expect(w.find("table").exists()).toBe(true);
    expect(w.findAll("tbody tr")).toHaveLength(2);
    await flush();
    expect(localStorage.getItem("forgevideo.poolView")).toBe("list");
    await w.find("button[aria-label='Thumbnail view']").trigger("click");
    expect(store.poolView).toBe("grid");
  });

  it("Import… opens a dialog filtered by the active tab and adds to the pool", async () => {
    const { w } = setup();
    await btn(w, "Clips").trigger("click");
    dialogOpen.mockResolvedValueOnce(["/v/1.mp4", "/v/2.mp4"]);
    await btn(w, "Import…").trigger("click");
    await flush();
    expect(dialogOpen).toHaveBeenCalledWith(expect.objectContaining({ multiple: true, filters: [expect.objectContaining({ name: "Clips" })] }));
    expect(api.poolAdd.mock.calls).toEqual([["/v/1.mp4"], ["/v/2.mp4"]]);
    await btn(w, "Images").trigger("click");
    dialogOpen.mockResolvedValueOnce("/i/x.png");
    await btn(w, "Import…").trigger("click");
    await flush();
    expect(dialogOpen).toHaveBeenLastCalledWith(expect.objectContaining({ filters: [expect.objectContaining({ extensions: expect.arrayContaining(["png"]) })] }));
    expect(api.poolAdd).toHaveBeenLastCalledWith("/i/x.png");
    dialogOpen.mockResolvedValueOnce(null);
    await btn(w, "Import…").trigger("click");
    await flush();
    expect(api.poolAdd).toHaveBeenCalledTimes(3);
  });

  it("double-click places an item at the playhead on its natural track; ✕ removes it", async () => {
    const { store, w } = setup();
    store.playhead = 1500;
    const cards = w.findAll("[data-testid=pool-items] > div > div");
    await cards[1].trigger("dblclick");
    expect(api.clipInsert).toHaveBeenCalledWith("/videos/b.mov", 1);
    await btn(w, "Images").trigger("click");
    await w.find("[data-testid=pool-items] > div > div").trigger("dblclick");
    expect(api.clipInsert).toHaveBeenLastCalledWith("/images/logo.png", 1);
    expect(api.overlayAdd).not.toHaveBeenCalled();
    // audio with no track yet: a Music track is created first
    await btn(w, "Audio").trigger("click");
    const withTrack = project([clip({ id: "a" })], { pool: items(), audio_tracks: [audioTrack([], { id: "t1" })] });
    api.audioTrackAdd.mockImplementationOnce(() => Promise.resolve(withTrack));
    await w.find("[data-testid=pool-items] > div > div").trigger("dblclick");
    await flush();
    expect(api.audioTrackAdd).toHaveBeenCalledWith("Music");
    expect(api.audioClipAdd).toHaveBeenCalledWith("t1", "/audio/bed.m4a", 1500);
    await w.find("button[aria-label='Remove from pool']").trigger("click");
    expect(api.poolRemove).toHaveBeenCalledWith("a1");
  });

  it("pointer-drags an item and dispatches a pooldrop on the row under the pointer", async () => {
    const { store, w } = setup();
    const card = w.find("[data-testid=pool-items] > div > div");
    const row = document.createElement("div");
    row.dataset.row = "video";
    const received = vi.fn();
    row.addEventListener("pooldrop", (e) => received((e as CustomEvent).detail));
    document.body.appendChild(row);
    const efp = vi.spyOn(document, "elementFromPoint").mockReturnValue(row);
    await card.trigger("pointerdown", { button: 0, clientX: 10, clientY: 10 });
    window.dispatchEvent(new PointerEvent("pointermove", { clientX: 12, clientY: 11 }));
    expect(store.poolDrag).toBeNull();
    window.dispatchEvent(new PointerEvent("pointermove", { clientX: 60, clientY: 40 }));
    expect(store.poolDrag?.item.id).toBe("v1");
    await w.vm.$nextTick();
    expect(w.text()).toContain("a.mp4");
    window.dispatchEvent(new PointerEvent("pointerup", { clientX: 200, clientY: 300 }));
    expect(store.poolDrag).toBeNull();
    expect(received).toHaveBeenCalledWith(expect.objectContaining({ clientX: 200, item: expect.objectContaining({ id: "v1" }) }));
    efp.mockRestore();
    row.remove();
  });

  it("Import… from the All tab offers every media extension", async () => {
    const { w } = setup();
    dialogOpen.mockResolvedValue(["/new/x.wav"]);
    await btn(w, "Import…").trigger("click");
    const ext = dialogOpen.mock.calls[0][0].filters[0].extensions as string[];
    expect(dialogOpen.mock.calls[0][0].filters[0].name).toBe("All");
    for (const e of ["mp4", "mov", "mp3", "wav", "png", "webp"]) expect(ext).toContain(e);
    await flush();
    expect(api.poolAdd).toHaveBeenCalledWith("/new/x.wav");
  });

  it("dropping audio inside the timeline with no audio track shows a banner instead of silently doing nothing", async () => {
    const { store, w } = setup();
    await btn(w, "Audio").trigger("click");
    const card = w.find("[data-testid=pool-items] > div > div");
    const tracks = document.createElement("div");
    tracks.dataset.testid = "timeline-tracks";
    const inner = document.createElement("div");
    tracks.appendChild(inner);
    document.body.appendChild(tracks);
    const efp = vi.spyOn(document, "elementFromPoint").mockReturnValue(inner);
    await card.trigger("pointerdown", { button: 0, clientX: 10, clientY: 10 });
    window.dispatchEvent(new PointerEvent("pointermove", { clientX: 60, clientY: 40 }));
    expect(store.poolDrag?.item.id).toBe("a1");
    window.dispatchEvent(new PointerEvent("pointerup", { clientX: 60, clientY: 40 }));
    expect(store.poolDrag).toBeNull();
    expect(store.notice).toBe("No audio track yet — click “+ Track”, then drop the audio there");
    expect(api.audioClipAdd).not.toHaveBeenCalled();
    // with a track present, a miss is just a miss
    store.notify(null);
    store.project = project([clip({ id: "a" })], { pool: items(), audio_tracks: [audioTrack([], { id: "t1" })] });
    await w.vm.$nextTick();
    await card.trigger("pointerdown", { button: 0, clientX: 10, clientY: 10 });
    window.dispatchEvent(new PointerEvent("pointermove", { clientX: 60, clientY: 40 }));
    window.dispatchEvent(new PointerEvent("pointerup", { clientX: 60, clientY: 40 }));
    expect(store.notice).toBeNull();
    // and a video card released off any row says nothing
    await btn(w, "Clips").trigger("click");
    const video = w.find("[data-testid=pool-items] > div > div");
    await video.trigger("pointerdown", { button: 0, clientX: 10, clientY: 10 });
    window.dispatchEvent(new PointerEvent("pointermove", { clientX: 60, clientY: 40 }));
    window.dispatchEvent(new PointerEvent("pointerup", { clientX: 60, clientY: 40 }));
    expect(store.notice).toBeNull();
    efp.mockRestore();
    tracks.remove();
  });
});
