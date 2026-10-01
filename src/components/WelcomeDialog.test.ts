import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { AI_READY, project, resolveWith, type MockApi } from "../test/fixtures";
import type { AiStatus } from "../types/project";

vi.mock("../api/tauri", async () => {
  const f = await import("../test/fixtures");
  return { api: f.mockApi(f.project([])) };
});
import { api as apiModule } from "../api/tauri";
const api = apiModule as unknown as MockApi;

import WelcomeDialog, { WELCOME_KEY } from "./WelcomeDialog.vue";
import { useProjectStore } from "../stores/project";

const flush = () => new Promise((r) => setTimeout(r, 0));
const NOTHING: AiStatus = { whisper: null, model: null, model_path: "/m/ggml-base.en.bin", claude: null, account: null };
const TOOLS_ONLY: AiStatus = { ...AI_READY, account: null };

async function setup(status: AiStatus) {
  setActivePinia(createPinia());
  resolveWith(api, project([]));
  api.aiStatus.mockImplementation(() => Promise.resolve({ ...status }));
  const w = mount(WelcomeDialog, { props: { open: true } });
  mounted.push(w);
  await flush();
  return { w, store: useProjectStore() };
}
const mounted: ReturnType<typeof mount>[] = [];
const btn = (w: ReturnType<typeof mount>, t: string) => w.findAll("button").find((b) => b.text().startsWith(t))!;
beforeEach(() => { vi.useRealTimers(); localStorage.clear(); for (const v of Object.values(api)) if (typeof v === "function" && "mockClear" in v) (v as ReturnType<typeof vi.fn>).mockClear(); });
afterEach(() => { mounted.splice(0).forEach((w) => w.unmount()); });

describe("WelcomeDialog", () => {
  it("renders nothing while closed", () => {
    setActivePinia(createPinia());
    const w = mount(WelcomeDialog, { props: { open: false } });
    expect(w.find("[role=dialog]").exists()).toBe(false);
  });

  it("with nothing installed: offers the installer and keeps sign-in disabled", async () => {
    const { w } = await setup(NOTHING);
    expect(w.find("[data-testid=welcome-tools]").text()).toContain("Install AI tools");
    const signIn = btn(w, "Sign in to Claude");
    expect(signIn.attributes("disabled")).toBeDefined();
    expect(w.find("[data-testid=welcome-account]").text()).toContain("once Claude Code is installed");
    expect(btn(w, "Skip for now")).toBeDefined();
  });

  it("Install AI tools… starts the installer and shows the waiting state", async () => {
    const { w, store } = await setup(NOTHING);
    await btn(w, "Install AI tools").trigger("click");
    expect(api.aiInstall).toHaveBeenCalled();
    expect(store.aiInstalling).toBe(true);
    expect(w.find("[data-testid=welcome-tools]").text()).toContain("Installing in the Terminal window");
    await btn(w, "Stop waiting").trigger("click");
    expect(store.aiInstalling).toBe(false);
  });

  it("with tools but no account: Sign in opens Terminal and polls until an account appears", async () => {
    const { w, store } = await setup(TOOLS_ONLY);
    expect(w.find("[data-testid=welcome-tools]").text()).toContain("✓");
    vi.useFakeTimers();
    api.aiStatus.mockImplementation(() => Promise.resolve({ ...AI_READY }));
    await btn(w, "Sign in to Claude").trigger("click");
    expect(api.aiClaudeLogin).toHaveBeenCalled();
    expect(store.aiSigningIn).toBe(true);
    expect(w.find("[data-testid=welcome-account]").text()).toContain("Finish signing in");
    await vi.advanceTimersByTimeAsync(2100);
    expect(store.aiSigningIn).toBe(false);
    await w.vm.$nextTick();
    expect(w.find("[data-testid=welcome-account]").text()).toContain(`Signed in as ${AI_READY.account}`);
    expect(btn(w, "Start editing")).toBeDefined();
    expect(w.findAll("button").some((b) => b.text() === "Skip for now")).toBe(false);
  });

  it("shows the sign-in error and still lets the user skip", async () => {
    const { w } = await setup(TOOLS_ONLY);
    api.aiClaudeLogin.mockImplementation(() => Promise.reject(new Error("no terminal")));
    await btn(w, "Sign in to Claude").trigger("click");
    await flush();
    expect(w.find("[data-testid=welcome-error]").text()).toContain("no terminal");
    await btn(w, "Skip for now").trigger("click");
    expect(w.emitted("close")).toHaveLength(1);
    expect(localStorage.getItem(WELCOME_KEY)).toBe("1");
  });
});
