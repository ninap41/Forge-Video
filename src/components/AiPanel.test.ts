import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { AI_READY, clip, cue, highlight, media, project, resolveWith, stillMedia, type MockApi } from "../test/fixtures";
import type { JobDone, JobError, JobProgress, Project } from "../types/project";

vi.mock("../api/tauri", async () => {
  const f = await import("../test/fixtures");
  return { api: f.mockApi(f.project([])) };
});
import { api as apiModule } from "../api/tauri";
const api = apiModule as unknown as MockApi;
const reveal = vi.fn();
vi.mock("@tauri-apps/plugin-opener", () => ({ revealItemInDir: (...a: unknown[]) => reveal(...(a as [])) }));

import AiPanel from "./AiPanel.vue";
import { useProjectStore } from "../stores/project";

const flush = () => new Promise((r) => setTimeout(r, 0));
let progressCb: (e: JobProgress) => void, doneCb: (e: JobDone) => void, errorCb: (e: JobError) => void;

const talk = () => clip({ id: "a", source: "/v/talk.mp4", media: media({ duration_ms: 120_000 }) });
const transcript = () => [{ source: "/v/talk.mp4", cues: [cue(0, 4000, "So here is the thing"), cue(4000, 9000, "nobody tells you this")] }];

async function setup(p: Project = project([talk()])) {
  setActivePinia(createPinia());
  const store = useProjectStore();
  store.project = p;
  resolveWith(api, p);
  const w = mount(AiPanel);
  mounted.push(w);
  await flush();
  return { store, w };
}
const mounted: ReturnType<typeof mount>[] = [];
afterEach(() => { vi.useRealTimers(); mounted.splice(0).forEach((w) => w.unmount()); });
const btn = (w: ReturnType<typeof mount>, t: string) => w.findAll("button").find((b) => b.text() === t)!;
const disabled = (w: ReturnType<typeof mount>, t: string) => btn(w, t).attributes("disabled") !== undefined;

beforeEach(() => {
  for (const v of Object.values(api)) if (typeof v === "function" && "mockClear" in v) (v as ReturnType<typeof vi.fn>).mockClear();
  api.aiStatus.mockImplementation(() => Promise.resolve({ ...AI_READY }));
  api.aiTranscribe.mockImplementation(() => Promise.resolve("job-t"));
  api.aiFindHighlights.mockImplementation(() => Promise.resolve("job-h"));
  api.highlightApply.mockImplementation(() => Promise.resolve("/v/Test - The hook.forgevideo"));
  api.onJobProgress.mockImplementation((cb: (e: JobProgress) => void) => { progressCb = cb; return Promise.resolve(() => {}); });
  api.onJobDone.mockImplementation((cb: (e: JobDone) => void) => { doneCb = cb; return Promise.resolve(() => {}); });
  api.onJobError.mockImplementation((cb: (e: JobError) => void) => { errorCb = cb; return Promise.resolve(() => {}); });
  reveal.mockReset();
});

describe("AiPanel", () => {
  it("hides setup when everything is installed and gates each step on the one before", async () => {
    const { w } = await setup();
    expect(api.aiStatus).toHaveBeenCalledTimes(1);
    expect(w.find("[data-testid=ai-setup]").exists()).toBe(false);
    expect(w.text()).toContain("Not transcribed yet");
    expect(disabled(w, "Transcribe")).toBe(false);
    expect(disabled(w, "Find highlights")).toBe(true);
  });

  it("lists what is missing and how to fix it", async () => {
    api.aiStatus.mockImplementation(() => Promise.resolve({ whisper: null, model: null, model_path: "/Users/m/Library/Application Support/ForgeVideo/models/ggml-base.en.bin", claude: "/opt/homebrew/bin/claude", account: null }));
    const { w } = await setup(project([talk()], { transcripts: transcript() }));
    const rows = w.findAll("[data-testid=ai-setup] li").map((li) => li.text());
    expect(rows[0]).toContain("✕");
    expect(rows[0]).toContain("brew install whisper.cpp");
    expect(rows[1]).toContain("ForgeVideo/models/ggml-base.en.bin");
    expect(rows[2]).toBe("✓Claude Code");
    expect(rows[3]).toContain("Sign in below");
    expect(disabled(w, "Transcribe")).toBe(true);
    expect(disabled(w, "Find highlights")).toBe(true);
  });

  it("shows the signed-in account and signs out", async () => {
    const { w } = await setup();
    const acct = w.find("[data-testid=ai-account]");
    expect(acct.text()).toContain("matt@example.com");
    expect(acct.text()).not.toContain("Sign in to Claude");
    api.aiStatus.mockImplementation(() => Promise.resolve({ ...AI_READY, account: null }));
    await btn(w, "Sign out").trigger("click");
    await flush();
    expect(api.aiClaudeLogout).toHaveBeenCalledTimes(1);
    expect(w.find("[data-testid=ai-account]").text()).toContain("Sign in to Claude");
    expect(disabled(w, "Sign in to Claude")).toBe(false);
  });

  it("signs in through Terminal and waits for the account to appear", async () => {
    api.aiStatus.mockImplementation(() => Promise.resolve({ ...AI_READY, account: null }));
    const { store, w } = await setup();
    vi.useFakeTimers();
    try {
      await btn(w, "Sign in to Claude").trigger("click");
      await vi.advanceTimersByTimeAsync(0);
      expect(api.aiClaudeLogin).toHaveBeenCalledTimes(1);
      expect(store.aiSigningIn).toBe(true);
      expect(w.find("[data-testid=ai-account]").text()).toContain("Terminal window");
      await vi.advanceTimersByTimeAsync(2000);
      expect(api.aiStatus).toHaveBeenCalledTimes(2);
      expect(store.aiSigningIn).toBe(true);
      api.aiStatus.mockImplementation(() => Promise.resolve({ ...AI_READY }));
      await vi.advanceTimersByTimeAsync(2000);
      expect(store.aiSigningIn).toBe(false);
      expect(w.find("[data-testid=ai-account]").text()).toContain("matt@example.com");
      expect(disabled(w, "Find highlights")).toBe(true); // still no captions
    } finally { vi.useRealTimers(); }
  });

  it("stops waiting on request and reports a Terminal that would not open", async () => {
    api.aiStatus.mockImplementation(() => Promise.resolve({ ...AI_READY, account: null }));
    const { store, w } = await setup();
    vi.useFakeTimers();
    try {
      await btn(w, "Sign in to Claude").trigger("click");
      await vi.advanceTimersByTimeAsync(0);
      await btn(w, "Stop waiting").trigger("click");
      await vi.advanceTimersByTimeAsync(2000);
      expect(store.aiSigningIn).toBe(false);
      expect(w.find("[data-testid=ai-account]").text()).toContain("Sign in to Claude");
      api.aiClaudeLogin.mockImplementationOnce(() => Promise.reject("could not open Terminal: no osascript"));
      await btn(w, "Sign in to Claude").trigger("click");
      await vi.advanceTimersByTimeAsync(0);
      expect(store.aiSigningIn).toBe(false);
      expect(w.find("[data-testid=ai-error]").text()).toContain("could not open Terminal");
    } finally { vi.useRealTimers(); }
  });

  it("offers the install script when a tool is missing and waits for it to land", async () => {
    api.aiStatus.mockImplementation(() => Promise.resolve({ ...AI_READY, whisper: null, model: null }));
    const { store, w } = await setup();
    expect(w.find("[data-testid=ai-install]").exists()).toBe(true);
    vi.useFakeTimers();
    try {
      await btn(w, "Install AI tools…").trigger("click");
      await vi.advanceTimersByTimeAsync(0);
      expect(api.aiInstall).toHaveBeenCalledTimes(1);
      expect(store.aiInstalling).toBe(true);
      expect(w.find("[data-testid=ai-install]").text()).toContain("Installing in the Terminal window");
      await vi.advanceTimersByTimeAsync(3000);
      expect(store.aiInstalling).toBe(true);
      api.aiStatus.mockImplementation(() => Promise.resolve({ ...AI_READY }));
      await vi.advanceTimersByTimeAsync(3000);
      expect(store.aiInstalling).toBe(false);
      expect(w.find("[data-testid=ai-install]").exists()).toBe(false);
      expect(w.find("[data-testid=ai-setup]").exists()).toBe(false);
      expect(disabled(w, "Transcribe")).toBe(false);
    } finally { vi.useRealTimers(); }
  });

  it("reports an install script that could not be started", async () => {
    api.aiStatus.mockImplementation(() => Promise.resolve({ ...AI_READY, claude: null, account: null }));
    api.aiInstall.mockImplementationOnce(() => Promise.reject("install script not found at /x/install-ai.sh"));
    const { store, w } = await setup();
    await btn(w, "Install AI tools…").trigger("click");
    await flush();
    expect(store.aiInstalling).toBe(false);
    expect(w.find("[data-testid=ai-error]").text()).toContain("install script not found");
  });

  it("disables sign-in until Claude Code is installed", async () => {
    api.aiStatus.mockImplementation(() => Promise.resolve({ ...AI_READY, claude: null, account: null }));
    const { w } = await setup();
    expect(disabled(w, "Sign in to Claude")).toBe(true);
    expect(w.find("[data-testid=ai-account]").text()).toContain("Install Claude Code first");
  });

  it("copies the transcript as plain text and confirms with a banner", async () => {
    const { store, w } = await setup(project([talk()], { transcripts: transcript() }));
    await btn(w, "Copy").trigger("click");
    await flush();
    expect(api.copyText).toHaveBeenCalledWith("So here is the thing\nnobody tells you this");
    expect(store.notice).toBe("Transcript copied (2 lines)");
    api.copyText.mockImplementationOnce(() => Promise.reject("denied"));
    await btn(w, "Copy").trigger("click");
    await flush();
    expect(store.notice).toBe("Could not copy: denied");
    const bare = await setup(project([talk()]));
    expect(bare.w.find("[data-testid=copy-transcript]").exists()).toBe(false);
  });

  it("says so when a transcript came back with no speech", async () => {
    const { w } = await setup(project([talk()], { transcripts: [{ source: "/v/talk.mp4", cues: [] }] }));
    expect(w.find("[data-testid=caption-status]").text()).toBe("Transcribed, but no speech was found");
    expect(disabled(w, "Transcribe")).toBe(true);
  });

  it("has nothing to transcribe without speech on V1", async () => {
    const { w } = await setup(project([clip({ media: stillMedia() }), clip({ media: media({ has_audio: false }) })]));
    expect(disabled(w, "Transcribe")).toBe(true);
    const empty = await setup(project([]));
    expect(empty.w.text()).toContain("Import a recording first");
  });

  it("transcribes with progress, then shows the caption count", async () => {
    const { store, w } = await setup();
    await btn(w, "Transcribe").trigger("click");
    await flush();
    expect(api.aiTranscribe).toHaveBeenCalledTimes(1);
    expect(store.aiJob).toEqual({ id: "job-t", kind: "transcribe", progress: 0, message: null });
    expect(disabled(w, "Transcribe")).toBe(true);
    progressCb({ job_id: "other", kind: "export", progress: 0.9, message: null });
    progressCb({ job_id: "job-t", kind: "transcribe", progress: 0.42, message: "talk.mp4" });
    await flush();
    expect(w.find("[data-testid=ai-progress]").text()).toContain("Transcribing talk.mp4");
    expect(w.find("[data-testid=ai-progress]").text()).toContain("42%");

    const done = project([talk()], { transcripts: transcript() });
    doneCb({ job_id: "job-t", kind: "transcribe", result: { project: done } });
    await flush();
    expect(store.aiJob).toBeNull();
    expect(store.dirty).toBe(true);
    expect(w.find("[data-testid=ai-progress]").exists()).toBe(false);
    expect(w.text()).toContain("2 captions");
    expect(store.notice).toBe("2 captions added");
    expect(disabled(w, "Transcribe")).toBe(true);
    expect(disabled(w, "Find highlights")).toBe(false);
  });

  it("offers to transcribe clips added after the first pass", async () => {
    const p = project([talk(), clip({ id: "b", source: "/v/extra.mp4" })], { transcripts: transcript() });
    const { w } = await setup(p);
    expect(disabled(w, "Transcribe new clips")).toBe(false);
  });

  it("cancels a running job and stays quiet about it", async () => {
    const { store, w } = await setup();
    await btn(w, "Transcribe").trigger("click");
    await flush();
    await btn(w, "Cancel").trigger("click");
    expect(api.jobCancel).toHaveBeenCalledWith("job-t");
    errorCb({ job_id: "job-t", kind: "transcribe", error: "cancelled" });
    await flush();
    expect(store.aiJob).toBeNull();
    expect(w.find("[data-testid=ai-error]").exists()).toBe(false);
  });

  it("shows failures from the job and from starting it", async () => {
    const { w } = await setup(project([talk()], { transcripts: transcript() }));
    await btn(w, "Find highlights").trigger("click");
    await flush();
    expect(w.find("[data-testid=ai-progress]").text()).toContain("Claude is reading the transcript");
    errorCb({ job_id: "job-h", kind: "highlights", error: "Claude could not answer: Please run /login" });
    await flush();
    expect(w.find("[data-testid=ai-error]").text()).toContain("Please run /login");
    api.aiFindHighlights.mockImplementationOnce(() => Promise.reject("Claude Code not found."));
    await btn(w, "Find highlights").trigger("click");
    await flush();
    expect(w.find("[data-testid=ai-error]").text()).toBe("Claude Code not found.");
    expect(w.find("[data-testid=ai-progress]").exists()).toBe(false);
  });

  it("accepts a result that arrives before the job id", async () => {
    let release: (id: string) => void = () => {};
    api.aiFindHighlights.mockImplementationOnce(() => new Promise<string>((r) => { release = r; }));
    const { store, w } = await setup(project([talk()], { transcripts: transcript() }));
    await btn(w, "Find highlights").trigger("click");
    await flush();
    expect(disabled(w, "Cancel")).toBe(true);
    doneCb({ job_id: "job-h", kind: "highlights", result: { project: project([talk()], { transcripts: transcript(), highlights: [highlight()] }) } });
    release("job-h");
    await flush();
    expect(store.aiJob).toBeNull();
    expect(store.notice).toBe("1 highlight found");
    expect(w.findAll("[data-testid=highlight]")).toHaveLength(1);
  });

  it("lists each highlight with its plan and seeks to it", async () => {
    const h = highlight({ id: "h1", title: "Why sleep matters", reason: "Hook and payoff", start: 12_000, end: 65_000, keep: [{ start: 12_000, end: 20_000 }, { start: 55_000, end: 65_000 }], fade_in: 300, fade_out: 800, notes: ["Add a title card"] });
    const { store, w } = await setup(project([talk()], { transcripts: transcript(), highlights: [h, highlight({ id: "h2", title: "Plain" })] }));
    const cards = w.findAll("[data-testid=highlight]");
    expect(cards).toHaveLength(2);
    expect(cards[0].text()).toContain("1. Why sleep matters");
    expect(cards[0].text()).toContain("0:12–1:05 · 53 s");
    expect(cards[0].text()).toContain("Hook and payoff");
    expect(cards[0].findAll("[data-testid=plan] li").map((l) => l.text())).toEqual([
      "New project in Shorts / Reels 9:16", "Keep 0:12–0:20, 0:55–1:05 (18 s)", "Cut 35 s of filler", "Fade in 0.3 s · Fade out 0.8 s", "Captions carried over",
    ]);
    expect(cards[0].text()).toContain("By hand: Add a title card");
    expect(cards[1].findAll("[data-testid=plan] li").map((l) => l.text())).toEqual(["New project in Shorts / Reels 9:16", "Keep 0:01–0:08 (7 s)", "Captions carried over"]);
    store.playing = true;
    await cards[0].find("button[title='Show on the timeline']").trigger("click");
    expect(store.playhead).toBe(12_000);
    expect(store.playing).toBe(false);
    expect(w.text()).toContain("Find again");
  });

  it("creates a short, then offers to open or reveal it", async () => {
    const { store, w } = await setup(project([talk()], { transcripts: transcript(), highlights: [highlight({ id: "h1" })] }));
    await btn(w, "Create short").trigger("click");
    await flush();
    expect(api.highlightApply).toHaveBeenCalledWith("h1");
    expect(store.notice).toBe("Created Test - The hook.forgevideo");
    expect(store.dirty).toBe(false);
    expect(w.text()).toContain("Test - The hook.forgevideo");
    expect(btn(w, "Create short")).toBeUndefined();
    await btn(w, "Open").trigger("click");
    expect(w.emitted("open-project")).toEqual([["/v/Test - The hook.forgevideo"]]);
    await btn(w, "Reveal in Finder").trigger("click");
    expect(reveal).toHaveBeenCalledWith("/v/Test - The hook.forgevideo");
  });

  it("explains why a short could not be created, and dismisses highlights", async () => {
    const { w } = await setup(project([talk()], { transcripts: transcript(), highlights: [highlight({ id: "h1", title: "Hook" })] }));
    api.highlightApply.mockImplementationOnce(() => Promise.reject("invalid edit: save this project first, shorts are created next to it"));
    await btn(w, "Create short").trigger("click");
    await flush();
    expect(w.find("[data-testid=ai-error]").text()).toContain("save this project first");
    expect(btn(w, "Create short")).toBeDefined();
    await w.find("button[aria-label='Dismiss Hook']").trigger("click");
    expect(api.highlightDelete).toHaveBeenCalledWith("h1");
  });
});
