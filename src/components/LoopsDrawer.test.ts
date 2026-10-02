import { beforeEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { clip, loopFx, project, resolveWith, type MockApi } from "../test/fixtures";

vi.mock("../api/tauri", async () => {
  const f = await import("../test/fixtures");
  return { api: f.mockApi(f.project([])) };
});
import { api as apiModule } from "../api/tauri";
const api = apiModule as unknown as MockApi;

import LoopsDrawer from "./LoopsDrawer.vue";
import { useProjectStore } from "../stores/project";

const flush = () => new Promise((r) => setTimeout(r, 0));

function setup(p = project([clip({ id: "a" })])) {
  setActivePinia(createPinia());
  const store = useProjectStore();
  store.project = p;
  resolveWith(api, p);
  const w = mount(LoopsDrawer);
  return { store, w };
}

beforeEach(() => {
  for (const v of Object.values(api)) if (typeof v === "function" && "mockClear" in v) (v as ReturnType<typeof vi.fn>).mockClear();
  localStorage.clear();
});

describe("LoopsDrawer", () => {
  it("lists pinned loops: play loops it, select shows it, export opens the dialog, unpin removes it", async () => {
    const l = loopFx({ id: "l1", name: "Hook", start: 1000, end: 4000 });
    const { store, w } = setup(project([clip({ id: "a" })], { loops: [l] }));
    expect(w.find("[data-testid=loops-toggle]").text()).toContain("Pinned loops (1)");
    expect(w.find("[data-testid=pin-range]").attributes("disabled")).toBeDefined();
    const row = w.find("[data-testid=loop]");
    expect(row.text()).toContain("Hook");
    expect(row.text()).toContain("0:01–0:04 · 3 s");
    await row.find("[data-testid=loop-play]").trigger("click");
    expect([store.playing, store.loopOn, store.playhead, store.range, store.activeLoopId]).toEqual([true, true, 1000, { start: 1000, end: 4000 }, "l1"]);
    await w.vm.$nextTick();
    expect(row.find("[data-testid=loop-play]").text()).toBe("❚❚");
    await row.find("[data-testid=loop-play]").trigger("click");
    expect(store.playing).toBe(false);
    await row.find("[data-testid=loop-name]").trigger("click");
    expect(store.activeLoopId).toBe("l1");
    await row.find("[data-testid=loop-export]").trigger("click");
    expect(store.exportOpen).toBe(true);
    await row.find("[data-testid=loop-unpin]").trigger("click");
    expect(api.loopRemove).toHaveBeenCalledWith("l1");
  });

  it("Pin range pins the current selection; collapsing hides the rows and persists", async () => {
    const { store, w } = setup(project([clip({ id: "a" })], { loops: [loopFx()] }));
    expect(w.find("[data-testid=loop]").exists()).toBe(true);
    store.setRange({ start: 0, end: 2000 });
    await w.vm.$nextTick();
    await w.find("[data-testid=pin-range]").trigger("click");
    expect(api.loopAdd).toHaveBeenCalledWith("", 0, 2000);
    await w.find("[data-testid=loops-toggle]").trigger("click");
    expect(w.find("[data-testid=loop]").exists()).toBe(false);
    await flush();
    expect(localStorage.getItem("forgevideo.loopsOpen")).toBe("0");
    expect(w.text()).toContain("Pinned loops");
  });

  it("double-click renames a loop; Enter / blur commit, Escape cancels, unchanged is a no-op", async () => {
    const l = loopFx({ id: "l1", name: "Hook" });
    const { w } = setup(project([clip({ id: "a" })], { loops: [l] }));
    await w.find("[data-testid=loop-name]").trigger("dblclick");
    const input = w.find("[data-testid=loop-rename]");
    expect((input.element as HTMLInputElement).value).toBe("Hook");
    await input.setValue("Cold open");
    await input.trigger("keydown", { key: "Enter" });
    expect(api.loopRename).toHaveBeenCalledWith("l1", "Cold open");
    expect(w.find("[data-testid=loop-rename]").exists()).toBe(false);
    await w.find("[data-testid=loop-name]").trigger("dblclick");
    await w.find("[data-testid=loop-rename]").setValue("discarded");
    await w.find("[data-testid=loop-rename]").trigger("keydown", { key: "Escape" });
    expect(api.loopRename).toHaveBeenCalledTimes(1);
    await w.find("[data-testid=loop-name]").trigger("dblclick");
    await w.find("[data-testid=loop-rename]").trigger("blur");
    expect(api.loopRename).toHaveBeenCalledTimes(1);
  });

  it("pinning a range that is already pinned shows a banner instead of a second loop", async () => {
    const l = loopFx({ id: "l1", name: "Hook", start: 1000, end: 4000 });
    const { store, w } = setup(project([clip({ id: "a" })], { loops: [l] }));
    store.setRange({ start: 1000, end: 4000 });
    await w.vm.$nextTick();
    await w.find("[data-testid=pin-range]").trigger("click");
    expect(api.loopAdd).not.toHaveBeenCalled();
    expect(store.notice).toBe("Already pinned as “Hook”");
    expect(store.activeLoopId).toBe("l1");
  });

  it("shows a hint when nothing is pinned", () => {
    const { w } = setup();
    expect(w.text()).toContain("Drag on the ruler to select a range, then Pin");
  });
});
