import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { baseProject, fakeSplit, poolItem, project, resolveWith, type MockApi } from "./test/fixtures";

vi.mock("./api/tauri", async () => {
  const f = await import("./test/fixtures");
  return { api: f.mockApi(f.baseProject()) };
});
import { api as apiModule } from "./api/tauri";
const api = apiModule as unknown as MockApi;
const dialog = { open: vi.fn(), save: vi.fn(), ask: vi.fn() };
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: (...a: unknown[]) => dialog.open(...(a as [])), save: (...a: unknown[]) => dialog.save(...(a as [])), ask: (...a: unknown[]) => dialog.ask(...(a as [])) }));
vi.mock("@tauri-apps/plugin-opener", () => ({ revealItemInDir: vi.fn() }));
let dropHandler: ((e: { payload: { type: string; paths: string[]; position?: { x: number; y: number } } }) => void) | undefined;
const unlistenDrop = vi.fn();
vi.mock("@tauri-apps/api/webview", () => ({ getCurrentWebview: () => ({ onDragDropEvent: (cb: typeof dropHandler) => { dropHandler = cb; return Promise.resolve(unlistenDrop); } }) }));
const setTitle = vi.fn();
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({ setTitle }) }));

import App from "./App.vue";
import { useProjectStore } from "./stores/project";
import { SHORTCUTS } from "./components/HelpDialog.vue";
import { WELCOME_KEY } from "./components/WelcomeDialog.vue";
import ClipBlock from "./components/ClipBlock.vue";

const base = baseProject();

const flush = () => new Promise((r) => setTimeout(r, 0));
const key = (init: KeyboardEventInit, target: EventTarget = window) => { target.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init })); };

async function setup() {
  setActivePinia(createPinia());
  const w = mount(App, { attachTo: document.body });
  mounted.push(w);
  await flush();
  return { w, store: useProjectStore() };
}
const mounted: ReturnType<typeof mount>[] = [];
afterEach(() => { mounted.splice(0).forEach((w) => w.unmount()); });
const btn = (w: ReturnType<typeof mount>, t: string) => w.findAll("button").find((b) => b.text().startsWith(t))!;

beforeEach(() => {
  for (const v of Object.values(api)) if (typeof v === "function" && "mockClear" in v) (v as ReturnType<typeof vi.fn>).mockClear();
  resolveWith(api, base);
  api.ffmpegStatus.mockImplementation(() => Promise.resolve({ ffmpeg: "/x/ffmpeg", ffprobe: "/x/ffprobe" }));
  Object.values(dialog).forEach((d) => d.mockReset());
  setTitle.mockClear(); unlistenDrop.mockClear(); dropHandler = undefined;
  localStorage.setItem(WELCOME_KEY, "1"); // every suite but "first launch" is past the welcome dialog
});

describe("App shell", () => {
  it("loads the project, sets the title and reports ffmpeg", async () => {
    const { w, store } = await setup();
    expect(api.projectGet).toHaveBeenCalled();
    expect(store.clips).toHaveLength(2);
    expect(setTitle).toHaveBeenCalledWith("ForgeVideo");
    expect(w.text()).not.toContain("ffmpeg not found");
    mounted.splice(0).forEach((m) => m.unmount());
    expect(unlistenDrop).toHaveBeenCalled();
  });

  it("warns when ffmpeg is missing", async () => {
    api.ffmpegStatus.mockImplementation(() => Promise.resolve({ ffmpeg: null, ffprobe: "/x/ffprobe" }));
    const { w } = await setup();
    expect(w.text()).toContain("ffmpeg not found — brew install ffmpeg");
  });

  it("marks unsaved work and surfaces store errors in the header", async () => {
    const { w, store } = await setup();
    expect(btn(w, "Save").text()).toBe("Save");
    await store.trim("a", 0, 1000);
    await w.vm.$nextTick();
    expect(btn(w, "Save").text()).toBe("Save •");
    store.error = "media error: boom";
    await w.vm.$nextTick();
    expect(w.text()).toContain("media error: boom");
  });

  it("Play and Export are disabled without clips", async () => {
    api.projectGet.mockImplementation(() => Promise.resolve(project([])));
    const { w } = await setup();
    expect(btn(w, "▶ Play").attributes("disabled")).toBeDefined();
    expect(btn(w, "Export…").attributes("disabled")).toBeDefined();
  });

  it("adds dropped media files to the pool, ignoring other types and hover events", async () => {
    const { store } = await setup();
    expect(dropHandler).toBeDefined();
    dropHandler!({ payload: { type: "enter", paths: ["/v/a.mp4"] } });
    dropHandler!({ payload: { type: "drop", paths: ["/v/a.MOV", "/docs/notes.txt", "/v/b.mkv", "/audio/x.m4a", "/img/logo.PNG"] } });
    await flush();
    expect(api.poolAdd.mock.calls).toEqual([["/v/a.MOV"], ["/v/b.mkv"], ["/audio/x.m4a"], ["/img/logo.PNG"]]);
    expect(api.mediaImport).not.toHaveBeenCalled();
    dropHandler!({ payload: { type: "drop", paths: ["/docs/notes.txt"] } });
    await flush();
    expect(api.poolAdd).toHaveBeenCalledTimes(4);
    expect(store.clips).toHaveLength(2);
  });

  it("a Finder drop of unsupported files says what was skipped, and still pools the media", async () => {
    const { store } = await setup();
    api.poolAdd.mockImplementation((path: string) => Promise.resolve(project(base.clips, { pool: [...store.pool, poolItem({ path })] })));
    dropHandler!({ payload: { type: "drop", paths: ["/docs/notes.txt", "/docs/deck.key", "/docs/a.pdf", "/audio/x.ogg"] } });
    await flush();
    expect(store.notice).toBe("Skipped notes.txt, deck.key +1 more — not a supported media file");
    expect(api.poolAdd.mock.calls).toEqual([["/audio/x.ogg"]]);
    store.notify(null);
    dropHandler!({ payload: { type: "drop", paths: ["/audio/y.opus", "/v/z.3gp"] } });
    await flush();
    expect(store.notice).toBeNull();
    expect(api.poolAdd).toHaveBeenCalledTimes(3);
  });

  it("a Finder drop that lands on the timeline still goes to the pool, with a banner saying so", async () => {
    const { w, store } = await setup();
    const tracks = w.find("[data-testid=timeline-tracks]").element;
    api.poolAdd.mockImplementation((path: string) => Promise.resolve(project(base.clips, { pool: [...store.pool, poolItem({ path })] })));
    const efp = vi.spyOn(document, "elementFromPoint").mockImplementation((x) => (x < 500 ? tracks : document.body));
    Object.defineProperty(window, "devicePixelRatio", { value: 2, configurable: true });
    dropHandler!({ payload: { type: "drop", paths: ["/v/c.mp4"], position: { x: 400, y: 900 } } });
    await flush();
    expect(efp).toHaveBeenCalledWith(200, 450);
    expect(api.poolAdd).toHaveBeenCalledWith("/v/c.mp4");
    expect(store.notice).toBe("Added to the media pool — drag from the pool onto the timeline");
    expect(w.find("[data-testid=banner]").text()).toContain("Added to the media pool");
    store.notify(null);
    dropHandler!({ payload: { type: "drop", paths: ["/v/d.mp4"], position: { x: 1600, y: 100 } } });
    await flush();
    expect(api.poolAdd).toHaveBeenCalledWith("/v/d.mp4");
    expect(store.notice).toBeNull();
    efp.mockRestore();
  });

  it("Import… button uses the file dialog for one or many files", async () => {
    const { w } = await setup();
    dialog.open.mockResolvedValueOnce(["/v/1.mp4", "/v/2.mp4"]);
    await btn(w, "Import…").trigger("click");
    await flush();
    expect(dialog.open).toHaveBeenCalledWith(expect.objectContaining({ multiple: true }));
    expect(api.mediaImport.mock.calls).toEqual([["/v/1.mp4"], ["/v/2.mp4"]]);
    dialog.open.mockResolvedValueOnce("/v/3.mp4");
    await btn(w, "Import…").trigger("click");
    await flush();
    expect(api.mediaImport).toHaveBeenLastCalledWith("/v/3.mp4");
    dialog.open.mockResolvedValueOnce(null);
    await btn(w, "Import…").trigger("click");
    await flush();
    expect(api.mediaImport).toHaveBeenCalledTimes(3);
  });

  it("New / Open confirm before discarding unsaved changes", async () => {
    const { w, store } = await setup();
    await btn(w, "New").trigger("click");
    await flush();
    expect(dialog.ask).not.toHaveBeenCalled();
    expect(api.projectNew).toHaveBeenCalledTimes(1);
    await store.trim("a", 0, 1000);
    dialog.ask.mockResolvedValueOnce(false);
    await btn(w, "New").trigger("click");
    await flush();
    expect(dialog.ask).toHaveBeenCalledWith("Discard unsaved changes?", expect.objectContaining({ kind: "warning" }));
    expect(api.projectNew).toHaveBeenCalledTimes(1);
    dialog.ask.mockResolvedValueOnce(true);
    dialog.open.mockResolvedValueOnce("/p/reel.forgevideo");
    await btn(w, "Open").trigger("click");
    await flush();
    expect(api.projectOpen).toHaveBeenCalledWith("/p/reel.forgevideo");
    expect(store.dirty).toBe(false);
  });

  it("Save writes in place when a path is known, otherwise asks for one", async () => {
    const { w } = await setup();
    await btn(w, "Save").trigger("click");
    await flush();
    expect(api.projectSave).toHaveBeenCalledWith(undefined);
    expect(dialog.save).not.toHaveBeenCalled();
    api.projectSave.mockImplementationOnce(() => Promise.reject("invalid edit: no save path"));
    dialog.save.mockResolvedValueOnce("/p/Test.forgevideo");
    await btn(w, "Save").trigger("click");
    await flush();
    expect(dialog.save).toHaveBeenCalledWith(expect.objectContaining({ defaultPath: "Test.forgevideo" }));
    expect(api.projectSave).toHaveBeenLastCalledWith("/p/Test.forgevideo");
  });

  it("opens the export dialog from the button and ⌘E", async () => {
    const { w } = await setup();
    expect(w.find("h2").exists()).toBe(false);
    await btn(w, "Export…").trigger("click");
    await flush();
    expect(w.find("h2").text()).toBe("Export");
    await w.find(".fixed").trigger("pointerdown");
    expect(w.find("h2").exists()).toBe(false);
    key({ key: "e", metaKey: true });
    await flush();
    expect(w.find("h2").text()).toBe("Export");
  });
});

describe("keyboard shortcuts", () => {
  it("space toggles playback only when there are clips", async () => {
    const { store } = await setup();
    key({ code: "Space" });
    expect(store.playing).toBe(true);
    key({ code: "Space" });
    expect(store.playing).toBe(false);
    store.project = project([]);
    key({ code: "Space" });
    expect(store.playing).toBe(false);
  });

  it("⌘T splits at the playhead and the timeline shows both halves; a bare S or T does nothing; ⌘S saves", async () => {
    const { store, w } = await setup();
    api.clipSplit.mockImplementation((id: string, at: number) => Promise.resolve(fakeSplit(store.project!, id, at)));
    store.select("a");
    store.playhead = 2000;
    key({ key: "s" });
    key({ key: "t" });
    await flush();
    expect(api.clipSplit).not.toHaveBeenCalled();
    key({ key: "t", metaKey: true });
    await flush();
    expect(api.clipSplit).toHaveBeenCalledWith("a", 2000);
    expect(store.clips.map((c) => [c.id, c.source_start, c.source_end, c.timeline_start])).toEqual([
      ["a", 0, 2000, 0], ["a-split", 2000, 5000, 2000], ["b", 0, 4000, 5000],
    ]);
    expect(store.selectedClipId).toBe("a-split");
    expect(store.error).toBeNull();
    await w.vm.$nextTick();
    expect(w.text()).toContain("3 clips");
    expect(w.findAllComponents(ClipBlock)).toHaveLength(3);
    // ctrl works like ⌘ on the same key
    store.playhead = 7000;
    key({ key: "t", ctrlKey: true });
    await flush();
    expect(store.clips).toHaveLength(4);
    // ⌘J joins the highlighted pieces; with one clip selected it does nothing
    key({ key: "j", metaKey: true });
    await flush();
    expect(api.clipMerge).not.toHaveBeenCalled();
    store.select({ kind: "clip", id: "a" });
    store.select({ kind: "clip", id: "a-split" }, true);
    api.clipMerge.mockResolvedValueOnce({ project: { ...store.project!, clips: store.clips.filter((c) => c.id !== "a-split") }, new_id: "a" });
    key({ key: "j", metaKey: true });
    await flush();
    expect(api.clipMerge).toHaveBeenCalledWith(["a", "a-split"]);
    expect(store.clips.map((c) => c.id)).toEqual(["a", "b", "b-split"]);
    expect(store.selectedClipId).toBe("a");
    key({ key: "s", metaKey: true });
    await flush();
    expect(api.projectSave).toHaveBeenCalled();
    expect(api.clipSplit).toHaveBeenCalledTimes(2);
  });

  it("the timeline panel is resizable and remembers its height", async () => {
    const { store, w } = await setup();
    const h0 = store.timelineHeight;
    const handle = w.find("[data-testid=timeline-resize]");
    await handle.trigger("pointerdown", { clientY: 500, pointerId: 1 });
    await handle.trigger("pointermove", { clientY: 440 });
    await handle.trigger("pointerup");
    expect(store.timelineHeight).toBe(h0 + 60);
    await handle.trigger("pointerdown", { clientY: 500, pointerId: 1 });
    await handle.trigger("pointermove", { clientY: 2000 });
    await handle.trigger("pointerup");
    expect(store.timelineHeight).toBe(200);
  });

  it("Backspace/Delete remove the selected clip only", async () => {
    const { store } = await setup();
    key({ key: "Backspace" });
    await flush();
    expect(api.clipDelete).not.toHaveBeenCalled();
    store.select("b");
    key({ key: "Delete" });
    await flush();
    expect(api.clipDelete).toHaveBeenCalledWith("b");
  });

  it("arrows nudge by a frame or a second and stop playback; Home/End jump", async () => {
    const { store } = await setup();
    store.playhead = 1000;
    store.playing = true;
    key({ key: "ArrowRight" });
    expect(store.playhead).toBe(1033);
    expect(store.playing).toBe(false);
    key({ key: "ArrowRight", shiftKey: true });
    expect(store.playhead).toBe(2033);
    key({ key: "ArrowLeft" });
    expect(store.playhead).toBe(2000);
    key({ key: "ArrowLeft", shiftKey: true });
    expect(store.playhead).toBe(1000);
    key({ key: "End" });
    expect(store.playhead).toBe(8999);
    key({ key: "Home" });
    expect(store.playhead).toBe(0);
    key({ key: "ArrowLeft" });
    expect(store.playhead).toBe(0);
  });

  it("⌘I opens the import dialog, ⌘O the project picker", async () => {
    await setup();
    dialog.open.mockResolvedValue(null);
    key({ key: "i", metaKey: true });
    await flush();
    expect(dialog.open).toHaveBeenLastCalledWith(expect.objectContaining({ multiple: true }));
    key({ key: "o", ctrlKey: true });
    await flush();
    expect(dialog.open).toHaveBeenLastCalledWith(expect.objectContaining({ filters: [{ name: "ForgeVideo project", extensions: ["forgevideo", "json"] }] }));
  });

  it("ignores keys typed into inputs", async () => {
    const { store } = await setup();
    const input = document.createElement("input");
    document.body.appendChild(input);
    key({ code: "Space" }, input);
    expect(store.playing).toBe(false);
    input.remove();
  });
});

describe("help dialog", () => {
  it("opens from the toolbar ? button and lists every shortcut", async () => {
    const { w } = await setup();
    expect(w.find("[role=dialog]").exists()).toBe(false);
    await w.find("button[aria-label='Help: keyboard shortcuts']").trigger("click");
    const dlg = w.find("[role=dialog]");
    expect(dlg.exists()).toBe(true);
    expect(dlg.find("h2").text()).toBe("Keyboard shortcuts");
    for (const s of SHORTCUTS) { expect(dlg.text()).toContain(s.keys); expect(dlg.text()).toContain(s.action); }
    expect(dlg.text()).toContain("Trim in / out point");
    await dlg.find("button[aria-label=Close]").trigger("click");
    expect(w.find("[role=dialog]").exists()).toBe(false);
  });

  it("toggles with ? and closes with Escape; other shortcuts are ignored while open", async () => {
    const { w, store } = await setup();
    key({ key: "?", shiftKey: true });
    await w.vm.$nextTick();
    expect(w.find("[role=dialog]").exists()).toBe(true);
    key({ code: "Space" });
    expect(store.playing).toBe(false);
    key({ key: "Escape" });
    await w.vm.$nextTick();
    expect(w.find("[role=dialog]").exists()).toBe(false);
    key({ key: "?" });
    await w.vm.$nextTick();
    key({ key: "?" });
    await w.vm.$nextTick();
    expect(w.find("[role=dialog]").exists()).toBe(false);
  });

  it("clicking the backdrop or Close closes it", async () => {
    const { w } = await setup();
    await w.find("button[aria-label='Help: keyboard shortcuts']").trigger("click");
    await w.find("[role=dialog]").element.parentElement!.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
    await w.vm.$nextTick();
    expect(w.find("[role=dialog]").exists()).toBe(false);
    await w.find("button[aria-label='Help: keyboard shortcuts']").trigger("click");
    await w.findAll("[role=dialog] button").find((b) => b.text() === "Close")!.trigger("click");
    expect(w.find("[role=dialog]").exists()).toBe(false);
  });
});

describe("AI mode", () => {
  it("the toolbar button swaps the inspector for the AI panel and back", async () => {
    const { w } = await setup();
    const toggle = w.find("[data-testid=ai-toggle]");
    expect(toggle.attributes("aria-pressed")).toBe("false");
    expect(w.find("[data-testid=ai-panel]").exists()).toBe(false);
    expect(api.aiStatus).not.toHaveBeenCalled();
    await toggle.trigger("click");
    await flush();
    expect(toggle.attributes("aria-pressed")).toBe("true");
    expect(w.find("[data-testid=ai-panel]").exists()).toBe(true);
    expect(w.text()).not.toContain("Output");
    expect(api.aiStatus).toHaveBeenCalledTimes(1);
    await toggle.trigger("click");
    expect(w.find("[data-testid=ai-panel]").exists()).toBe(false);
    expect(w.text()).toContain("Output");
  });

  it("opening a created short asks before discarding unsaved changes", async () => {
    const { w, store } = await setup();
    store.project = { ...base, highlights: [{ id: "h1", title: "Hook", reason: "", start: 0, end: 4000, keep: [{ start: 0, end: 4000 }], fade_in: 0, fade_out: 0, notes: [] }] };
    await w.find("[data-testid=ai-toggle]").trigger("click");
    await flush();
    await btn(w, "Create short").trigger("click");
    await flush();
    api.projectOpen.mockClear();
    store.dirty = true;
    dialog.ask.mockResolvedValueOnce(false);
    await btn(w, "Open").trigger("click");
    await flush();
    expect(dialog.ask).toHaveBeenCalled();
    expect(api.projectOpen).not.toHaveBeenCalled();
    dialog.ask.mockResolvedValueOnce(true);
    await w.findAll("[data-testid=ai-panel] button").find((b) => b.text() === "Open")!.trigger("click");
    await flush();
    expect(api.projectOpen).toHaveBeenCalledWith("/videos/Test - The hook.forgevideo");
    expect(store.dirty).toBe(false);
  });
});

describe("first launch", () => {
  it("shows the welcome dialog once, then remembers that it was dismissed", async () => {
    localStorage.removeItem(WELCOME_KEY);
    const { w, store } = await setup();
    const dlg = w.find("[data-testid=welcome]");
    expect(dlg.exists()).toBe(true);
    expect(api.aiStatus).toHaveBeenCalled();
    key({ code: "Space" });
    expect(store.playing).toBe(false); // shortcuts wait until the dialog is gone
    await btn(w, "Start editing").trigger("click");
    expect(w.find("[data-testid=welcome]").exists()).toBe(false);
    expect(localStorage.getItem(WELCOME_KEY)).toBe("1");
    mounted.splice(0).forEach((m) => m.unmount());
    const again = await setup();
    expect(again.w.find("[data-testid=welcome]").exists()).toBe(false);
  });

  it("offers Skip for now when there is no Claude account yet", async () => {
    localStorage.removeItem(WELCOME_KEY);
    api.aiStatus.mockImplementation(() => Promise.resolve({ whisper: null, model: null, model_path: "/m", claude: null, account: null }));
    const { w } = await setup();
    await btn(w, "Skip for now").trigger("click");
    expect(w.find("[data-testid=welcome]").exists()).toBe(false);
    expect(localStorage.getItem(WELCOME_KEY)).toBe("1");
  });
});
