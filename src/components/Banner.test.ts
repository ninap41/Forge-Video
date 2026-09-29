import { afterEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";

vi.mock("../api/tauri", async () => {
  const f = await import("../test/fixtures");
  return { api: f.mockApi(f.project([])) };
});
import Banner from "./Banner.vue";
import { useProjectStore } from "../stores/project";

const mounted: ReturnType<typeof mount>[] = [];
afterEach(() => { mounted.splice(0).forEach((w) => w.unmount()); vi.useRealTimers(); });

describe("Banner", () => {
  it("is hidden until notified, shows the text, and dismisses on click or after the timeout", async () => {
    vi.useFakeTimers();
    setActivePinia(createPinia());
    const store = useProjectStore();
    const w = mount(Banner);
    mounted.push(w);
    expect(w.find("[data-testid=banner]").exists()).toBe(false);
    store.notify("Audio goes on an audio track");
    await w.vm.$nextTick();
    expect(w.find("[role=status]").text()).toBe("Audio goes on an audio track");
    await w.find("[data-testid=banner]").trigger("click");
    expect(store.notice).toBeNull();
    expect(w.find("[data-testid=banner]").exists()).toBe(false);
    store.notify("again", 500);
    await w.vm.$nextTick();
    expect(w.find("[data-testid=banner]").exists()).toBe(true);
    vi.advanceTimersByTime(600);
    await w.vm.$nextTick();
    expect(w.find("[data-testid=banner]").exists()).toBe(false);
  });
});
