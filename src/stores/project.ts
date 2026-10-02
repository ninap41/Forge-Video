import { defineStore } from "pinia";
import { ref, computed, watch } from "vue";
import { api } from "../api/tauri";
import { timelineCues, transcriptText, cueAt } from "../utils/captions";
import { rasterBase64, rasterText } from "../utils/textRaster";
import {
  type AiStatus, type AspectPreset, type AudioClip, type AudioTrack, type Crop, type LayerAudio, type MediaInfo, type Ms, type OverlayClip,
  type Placement, type PoolItem, type Project, type Range, type TextClip, type TextRaster, type TextStyle, type Transition,
  LAYER_AUDIO_DEFAULT, clipDuration, clipEnd, mediaKind, projectDuration, sameRange, textEnd,
} from "../types/project";

export interface Thumbs { intervalMs: number; urls: string[] }

/** What the user has selected: a V1 clip, an overlay, or an audio clip (with its track). */
export type Selection =
  | { kind: "clip"; id: string } | { kind: "overlay"; id: string } | { kind: "audio"; id: string; trackId: string }
  | { kind: "text"; id: string } | { kind: "captions"; id: "captions" };
export const CAPTIONS_SELECTION: Selection = { kind: "captions", id: "captions" };

export type PoolView = "grid" | "list";

/** The one AI job that can run at a time. `id` is null until Rust has answered with it. */
export interface AiJob { id: string | null; kind: "transcribe" | "highlights"; progress: number; message: string | null }

function readLocal<T extends string>(key: string, fallback: T): T {
  try { return (localStorage.getItem(key) as T | null) ?? fallback; } catch { return fallback; }
}
function writeLocal(key: string, value: string) {
  try { localStorage.setItem(key, value); } catch { /* private window, blocked storage: ignore */ }
}

export const useProjectStore = defineStore("project", () => {
  const project = ref<Project | null>(null);
  const selected = ref<Selection | null>(null);
  /** ⇧-click adds clips of the same row to the selection (for Merge); `selected` stays the primary. */
  const extraSelected = ref<Selection[]>([]);
  const sameRow = (a: Selection, b: Selection) => a.kind === b.kind && (a.kind !== "audio" || b.kind !== "audio" || a.trackId === b.trackId);
  const sameSel = (a: Selection, b: Selection) => sameRow(a, b) && a.id === b.id;
  const selectedAll = computed<Selection[]>(() => (selected.value ? [selected.value, ...extraSelected.value] : []));
  const playhead = ref<Ms>(0);
  const playing = ref(false);
  const dirty = ref(false);
  const error = ref<string | null>(null);
  const thumbs = ref<Record<string, Thumbs>>({}); // keyed by source path
  const waveforms = ref<Record<string, number[]>>({}); // keyed by source path, 1 byte per 10 ms
  const busy = ref<Record<string, boolean>>({});
  /** Media pool item being dragged onto the timeline (pointer-driven, not HTML5 DnD). */
  const poolDrag = ref<{ item: PoolItem; x: number; y: number } | null>(null);
  const poolView = ref<PoolView>(readLocal("forgevideo.poolView", "grid"));
  /** Short-lived banner text (rejected drops, duplicate imports); see Banner.vue. */
  const notice = ref<string | null>(null);
  let noticeTimer: ReturnType<typeof setTimeout> | undefined;
  function notify(text: string | null, ms = 3000) {
    if (noticeTimer) clearTimeout(noticeTimer);
    notice.value = text;
    if (text) noticeTimer = setTimeout(() => { notice.value = null; }, ms);
  }
  const timelineHeight = ref(Number(readLocal("forgevideo.timelineHeight", "360")) || 360);
  watch(poolView, (v) => writeLocal("forgevideo.poolView", v));
  watch(timelineHeight, (v) => writeLocal("forgevideo.timelineHeight", String(v)));
  /** The Pinned loops drawer under the tracks. */
  const loopsOpen = ref(readLocal("forgevideo.loopsOpen", "1") === "1");
  watch(loopsOpen, (v) => writeLocal("forgevideo.loopsOpen", v ? "1" : "0"));
  /** Where the project was last saved or opened from; reopened on launch and the default folder for Open / Save. */
  const lastProjectPath = ref<string | null>(readLocal("forgevideo.lastProject", "") || null);
  function rememberPath(path: string | null) {
    lastProjectPath.value = path;
    writeLocal("forgevideo.lastProject", path ?? "");
  }
  /** The Export dialog; lives here so the loops drawer and the AI panel can open it. */
  const exportOpen = ref(false);
  /** ✦ AI: the panel replaces the Inspector and the green highlight bands show on the timeline. */
  const aiOpen = ref(false);
  /** Splitters: the library column's width and the Pinned loops drawer's height, both remembered. */
  const libraryWidth = ref(Number(readLocal("forgevideo.libraryWidth", "288")) || 288);
  const loopsHeight = ref(Number(readLocal("forgevideo.loopsHeight", "120")) || 120);
  watch(libraryWidth, (v) => writeLocal("forgevideo.libraryWidth", String(v)));
  watch(loopsHeight, (v) => writeLocal("forgevideo.loopsHeight", String(v)));
  /** Bumped whenever a loop is pinned or picked, so the timeline scrolls to show the range. */
  const revealRange = ref(0);

  // ---- range selection (session only) and pinned loops (in the project) ----
  /** The yellow span selected on the ruler, in timeline ms. */
  const range = ref<Range | null>(null);
  /** When on, playback wraps from `range.end` back to `range.start`. */
  const loopOn = ref(false);
  /** The pinned loop the range came from, if any. Editing the range clears it. */
  const activeLoopId = ref<string | null>(null);

  const clips = computed(() => project.value?.clips ?? []);
  const overlays = computed(() => project.value?.overlays ?? []);
  const audioTracks = computed(() => project.value?.audio_tracks ?? []);
  const texts = computed(() => project.value?.texts ?? []);
  const textLayers = computed(() => Math.max(1, project.value?.text_layers ?? 1));
  const pool = computed(() => project.value?.pool ?? []);
  const duration = computed(() => (project.value ? projectDuration(project.value) : 0));
  const loops = computed(() => project.value?.loops ?? []);
  const activeLoop = computed(() => loops.value.find((l) => l.id === activeLoopId.value) ?? null);
  /** What the preview wraps on: the range, only while looping is on. */
  const loopRange = computed<Range | null>(() => (loopOn.value && range.value ? range.value : null));

  /** V1 selection, kept as a writable id so existing callers and tests keep working. */
  const selectedClipId = computed<string | null>({
    get: () => (selected.value?.kind === "clip" ? selected.value.id : null),
    set: (id) => { selected.value = id ? { kind: "clip", id } : null; extraSelected.value = []; },
  });
  const selectedClip = computed(() => clips.value.find((c) => c.id === selectedClipId.value) ?? null);
  const selectedIndex = computed(() => clips.value.findIndex((c) => c.id === selectedClipId.value));
  const selectedOverlay = computed(() => (selected.value?.kind === "overlay" ? overlays.value.find((o) => o.id === selected.value!.id) ?? null : null));
  const selectedText = computed(() => (selected.value?.kind === "text" ? texts.value.find((t) => t.id === selected.value!.id) ?? null : null));
  const selectedAudio = computed<{ clip: AudioClip; track: AudioTrack } | null>(() => {
    const s = selected.value; if (s?.kind !== "audio") return null;
    const track = audioTracks.value.find((t) => t.id === s.trackId); if (!track) return null;
    const c = track.clips.find((c) => c.id === s.id); return c ? { clip: c, track } : null;
  });

  /** Clip under the playhead and matching source time; mirrors timeline::locate in Rust. */
  const current = computed(() => {
    const t = playhead.value;
    for (let i = clips.value.length - 1; i >= 0; i--) {
      const c = clips.value[i];
      if (t >= c.timeline_start) {
        const off = Math.min(t - c.timeline_start, Math.max(clipDuration(c) - 1, 0));
        return { index: i, clip: c, sourceMs: c.source_start + off };
      }
    }
    return null;
  });
  /** Overlays under the playhead, lowest layer first; mirrors timeline::locate_overlays. */
  const overlayLayers = computed(() => Math.max(1, project.value?.overlay_layers ?? 1));
  /** Mute / fader of an overlay row; rows a file predates read as on at 100 %. */
  const layerAudio = (layer: number): LayerAudio => project.value?.overlay_audio?.[layer] ?? LAYER_AUDIO_DEFAULT;
  /** Overlays under the playhead, with what the preview should play them at (0 = silent). */
  const currentOverlays = computed<{ clip: OverlayClip; sourceMs: Ms; gain: number }[]>(() => {
    const t = playhead.value;
    return overlays.value.filter((o) => t >= o.timeline_start && t < clipEnd(o)).map((o) => {
      const row = layerAudio(o.layer);
      const silent = !o.media.has_audio || o.media.is_still || o.muted || row.muted;
      return { clip: o, sourceMs: o.source_start + (t - o.timeline_start), gain: silent ? 0 : Math.min(1, o.volume * row.volume) };
    });
  });
  /** Titles under the playhead with their fade opacity, lowest row first; mirrors timeline::locate_texts. */
  const currentTexts = computed<{ clip: TextClip; opacity: number }[]>(() => {
    const t = playhead.value;
    return [...texts.value].sort((a, b) => a.layer - b.layer || a.timeline_start - b.timeline_start).filter((x) => t >= x.timeline_start && t < textEnd(x)).map((x) => {
      const off = t - x.timeline_start;
      const fi = x.fade_in > 0 ? Math.min(1, off / x.fade_in) : 1;
      const fo = x.fade_out > 0 ? Math.min(1, (x.duration - off) / x.fade_out) : 1;
      return { clip: x, opacity: Math.max(0, Math.min(fi, fo)) };
    });
  });
  /** Font families for the Inspector's picker; fetched once, on first use. */
  const fonts = ref<string[]>([]);
  let fontsLoading: Promise<void> | null = null;
  function loadFonts() {
    fontsLoading ??= api.systemFonts().then((f) => { fonts.value = f; }).catch((e) => { console.warn("fonts", e); fontsLoading = null; });
    return fontsLoading;
  }
  /** Every audio clip that covers the playhead, with mute resolved against its track. */
  const activeAudioClips = computed<{ clip: AudioClip; track: AudioTrack; sourceMs: Ms; silent: boolean }[]>(() => {
    const t = playhead.value;
    const out: { clip: AudioClip; track: AudioTrack; sourceMs: Ms; silent: boolean }[] = [];
    for (const track of audioTracks.value) {
      for (const c of track.clips) {
        if (t >= c.timeline_start && t < clipEnd(c)) out.push({ clip: c, track, sourceMs: c.source_start + (t - c.timeline_start), silent: track.muted || c.muted });
      }
    }
    return out;
  });

  // ---- AI mode ----
  const cues = computed(() => (project.value ? timelineCues(project.value) : []));
  const captionsEnabled = computed(() => project.value?.captions_enabled ?? true);
  const selectedCaptions = computed(() => selected.value?.kind === "captions");
  /** The caption to draw in the preview: none while the captions track is off. */
  const currentCue = computed(() => (captionsEnabled.value ? cueAt(cues.value, playhead.value) : null));
  const highlights = computed(() => project.value?.highlights ?? []);
  const aiStatus = ref<AiStatus | null>(null);
  const aiJob = ref<AiJob | null>(null);
  const aiError = ref<string | null>(null);
  /** True while a Terminal sign-in is open and the store is polling for the account. */
  const aiSigningIn = ref(false);
  /** True while the install script runs in Terminal and the store is polling for the tools. */
  const aiInstalling = ref(false);

  /** Start a Terminal step, then poll `ai_status` every `every` ms (at most `tries` times) until `until` holds. */
  async function terminalStep(flag: typeof aiSigningIn, start: () => Promise<void>, until: (s: AiStatus) => boolean, every: number, tries: number) {
    aiError.value = null;
    flag.value = true;
    try {
      await start();
      for (let i = 0; i < tries && flag.value; i++) {
        await new Promise((r) => setTimeout(r, every));
        aiStatus.value = await api.aiStatus();
        if (until(aiStatus.value)) break;
      }
    } catch (e) {
      aiError.value = String(e);
    } finally {
      flag.value = false;
    }
  }

  function stillSelected(p: Project, s: Selection | null): boolean {
    if (!s) return false;
    if (s.kind === "clip") return p.clips.some((c) => c.id === s.id);
    if (s.kind === "overlay") return p.overlays.some((o) => o.id === s.id);
    if (s.kind === "captions") return timelineCues(p).length > 0;
    if (s.kind === "text") return p.texts.some((t) => t.id === s.id);
    return p.audio_tracks.some((t) => t.id === s.trackId && t.clips.some((c) => c.id === s.id));
  }

  function apply(p: Project, markDirty = true) {
    project.value = p;
    if (markDirty) dirty.value = true;
    if (!stillSelected(p, selected.value)) selected.value = null;
    extraSelected.value = extraSelected.value.filter((s) => stillSelected(p, s));
    playhead.value = Math.min(playhead.value, Math.max(projectDuration(p) - 1, 0));
    if (activeLoopId.value && !p.loops.some((l) => l.id === activeLoopId.value)) activeLoopId.value = null;
    if (range.value) {
      const total = projectDuration(p);
      const r = { start: Math.min(range.value.start, total), end: Math.min(range.value.end, total) };
      if (r.end - r.start < 100) setRange(null);
      else if (!sameRange(r, range.value)) range.value = r;
    }
  }

  /** Select a span on the ruler (sorted and clamped); `null` clears it and stops looping. */
  function setRange(r: Range | null) {
    activeLoopId.value = null;
    if (!r) { range.value = null; loopOn.value = false; return; }
    const total = Math.max(duration.value, 0);
    const start = Math.max(0, Math.min(r.start, r.end, total));
    const end = Math.min(Math.max(r.start, r.end), total);
    range.value = end - start < 100 ? null : { start, end };
    if (!range.value) loopOn.value = false;
  }
  /** Load a pinned loop: it becomes the range, looping on, ready for Space to run it from the top. */
  function selectLoop(id: string) {
    const l = loops.value.find((l) => l.id === id); if (!l) return;
    setRange({ start: l.start, end: l.end });
    activeLoopId.value = id;
    loopOn.value = true;
    revealRange.value++;
  }
  /** Space: pause, or play. With a loop loaded, play starts from the loop's beginning (GarageBand's cycle). */
  function togglePlay() {
    if (playing.value) { playing.value = false; return; }
    const r = loopRange.value;
    if (r) seek(r.start);
    playing.value = true;
  }
  function seek(ms: Ms) { playhead.value = Math.max(0, Math.min(ms, Math.max(duration.value - 1, 0))); }

  async function run<T>(label: string, fn: () => Promise<T>): Promise<T | undefined> {
    busy.value[label] = true;
    error.value = null;
    try {
      return await fn();
    } catch (e) {
      error.value = String(e);
      console.error(label, e);
      return undefined;
    } finally {
      busy.value[label] = false;
    }
  }

  function ensureMediaCaches(source: string, media: MediaInfo) {
    const kind = mediaKind(media);
    if (kind !== "Audio" && !thumbs.value[source]) {
      void api.cacheThumbnails(source, media.duration_ms).then((t) => {
        thumbs.value[source] = { intervalMs: t.interval_ms, urls: t.files.map(api.assetUrl) };
      }).catch((e) => console.warn("thumbs", e));
    }
    if (media.has_audio && !waveforms.value[source]) {
      void api.cacheWaveform(source).then((w) => { waveforms.value[source] = w.peaks; }).catch((e) => console.warn("waveform", e));
    }
  }
  function cacheAll(p: Project) {
    p.clips.forEach((c) => ensureMediaCaches(c.source, c.media));
    p.overlays.forEach((o) => ensureMediaCaches(o.source, o.media));
    p.audio_tracks.forEach((t) => t.clips.forEach((c) => ensureMediaCaches(c.source, c.media)));
    p.pool.forEach((i) => ensureMediaCaches(i.path, i.media));
  }

  /** Apply a project-returning command; returns whether it succeeded. */
  async function edit(label: string, fn: () => Promise<Project>): Promise<boolean> {
    const p = await run(label, fn);
    if (p) { apply(p); cacheAll(p); }
    return !!p;
  }

  const mine = (e: { job_id: string; kind: string }) => {
    const j = aiJob.value;
    return !!j && (j.id === e.job_id || (j.id === null && j.kind === e.kind));
  };
  let aiListening: Promise<unknown> | null = null;
  /** Subscribed on first use, so the store stays silent until AI mode runs something. */
  function listenAi() {
    aiListening ??= Promise.all([
      api.onJobProgress((e) => { if (mine(e)) aiJob.value = { ...aiJob.value!, progress: e.progress, message: e.message }; }),
      api.onJobDone((e) => {
        if (!mine(e) || !("project" in e.result)) return;
        const kind = aiJob.value!.kind;
        aiJob.value = null;
        apply(e.result.project); cacheAll(e.result.project);
        notify(kind === "transcribe" ? `${cues.value.length} captions added` : `${highlights.value.length} highlight${highlights.value.length === 1 ? "" : "s"} found`);
      }),
      api.onJobError((e) => {
        if (!mine(e)) return;
        aiJob.value = null;
        aiError.value = e.error === "cancelled" ? null : e.error;
      }),
    ]);
    return aiListening;
  }
  async function startAi(kind: AiJob["kind"], fn: () => Promise<string>) {
    if (aiJob.value) return;
    aiError.value = null;
    aiJob.value = { id: null, kind, progress: 0, message: null };
    try {
      await listenAi();
      const id = await fn();
      if (aiJob.value?.kind === kind && aiJob.value.id === null) aiJob.value = { ...aiJob.value, id };
    } catch (e) {
      aiJob.value = null;
      aiError.value = String(e);
    }
  }

  return {
    cues, currentCue, captionsEnabled, selectedCaptions, highlights, aiStatus, aiJob, aiError, aiSigningIn, aiInstalling,
    async setCaptionsEnabled(enabled: boolean) { await edit("captions", () => api.setCaptionsEnabled(enabled)); },
    /** Copy the timeline transcript as plain text; a banner confirms or explains. */
    async copyTranscript() {
      const text = transcriptText(cues.value);
      if (!text) { notify("Nothing to copy: transcribe first"); return; }
      try {
        await api.copyText(text);
        notify(`Transcript copied (${cues.value.length} lines)`);
      } catch (e) {
        notify(`Could not copy: ${String(e)}`);
      }
    },
    async refreshAiStatus() { aiStatus.value = (await run("ai", () => api.aiStatus())) ?? null; },
    /** Sign-in happens in Terminal + browser; poll every 2 s (up to 5 min) until Claude reports an account. */
    claudeLogin: () => terminalStep(aiSigningIn, () => api.aiClaudeLogin(), (s) => !!s.account, 2000, 150),
    cancelClaudeLogin() { aiSigningIn.value = false; },
    /** The install script runs in Terminal; poll every 3 s (up to 20 min) until whisper, model and Claude Code are all there. */
    installAi: () => terminalStep(aiInstalling, () => api.aiInstall(), (s) => !!s.whisper && !!s.model && !!s.claude, 3000, 400),
    cancelInstallAi() { aiInstalling.value = false; },
    async claudeLogout() {
      aiError.value = null;
      try {
        await api.aiClaudeLogout();
        aiStatus.value = await api.aiStatus();
      } catch (e) {
        aiError.value = String(e);
      }
    },
    transcribe: () => startAi("transcribe", () => api.aiTranscribe()),
    findHighlights: () => startAi("highlights", () => api.aiFindHighlights()),
    async cancelAi() { if (aiJob.value?.id) await api.jobCancel(aiJob.value.id); },
    async setCueText(id: string, text: string) { await edit("caption", () => api.cueSetText(id, text)); },
    async deleteHighlight(id: string) { await edit("highlight", () => api.highlightDelete(id)); },

    project, clips, overlays, texts, audioTracks, pool, duration, selected, selectedAll, selectedClipId, selectedClip, selectedIndex, selectedOverlay, selectedText, selectedAudio,
    playhead, playing, dirty, error, thumbs, waveforms, busy, current, currentOverlays, currentTexts, fonts, overlayLayers, layerAudio, activeAudioClips, poolDrag, poolView, timelineHeight, notice, notify,

    /** Launch: reopen the last project when it still opens, otherwise the fresh one Rust started with. */
    async load() {
      const last = lastProjectPath.value;
      if (last) {
        const p = await api.projectOpen(last).catch(() => null);
        if (p) { apply(p, false); cacheAll(p); return; }
        rememberPath(null);
      }
      const p = await api.projectGet(); apply(p, false); cacheAll(p);
    },
    async newProject() { apply(await api.projectNew("Untitled"), false); dirty.value = false; playhead.value = 0; rememberPath(null); },
    async open(path: string) {
      const ok = await run("open", async () => { const p = await api.projectOpen(path); apply(p, false); cacheAll(p); return true; });
      dirty.value = false;
      if (ok) rememberPath(path);
    },
    async save(path?: string) { const r = await run("save", () => api.projectSave(path)); if (r) { dirty.value = false; rememberPath(r); } return r; },
    lastProjectPath,

    /** Import: video also lands on V1 (and is selected); audio and images go to the pool only. */
    async importMedia(paths: string[]) {
      for (const path of paths) {
        const before = clips.value.length;
        const p = await run("import", () => api.mediaImport(path));
        if (!p) continue;
        apply(p); cacheAll(p);
        const c = p.clips[p.clips.length - 1];
        if (c && p.clips.length > before) selected.value = { kind: "clip", id: c.id };
      }
    },
    /** Pool only. A file already in the pool (same path, or the same file via a symlink) is skipped with a banner. */
    async poolAdd(paths: string[]) {
      for (const path of paths) {
        const dup = () => notify(`${path.split("/").pop()} is already in the media pool`);
        if (pool.value.some((i) => i.path === path)) { dup(); continue; }
        const before = pool.value.length;
        if (await edit("import", () => api.poolAdd(path)) && pool.value.length === before) dup();
      }
    },
    async poolRemove(id: string) { await edit("pool", () => api.poolRemove(id)); },
    async insertClip(path: string, atIndex: number) { await edit("insert", () => api.clipInsert(path, atIndex)); },

    async trim(id: string, s: Ms, e: Ms) { await edit("trim", () => api.clipTrim(id, s, e)); },
    /** ⌘T: split whatever is selected at the playhead, falling back to the V1 clip under it. */
    /**
     * ⌘T. Splits the selected overlay/audio clip if the playhead is inside it; otherwise the V1
     * clip under the playhead. Selects the new right-hand half.
     */
    async splitAtPlayhead() {
      const at = playhead.value;
      const s = selected.value;
      const inside = (c: { timeline_start: Ms; source_start: Ms; source_end: Ms }) => at > c.timeline_start && at < clipEnd(c);
      const target: Selection | null =
        s?.kind === "overlay" && selectedOverlay.value && inside(selectedOverlay.value) ? s
        : s?.kind === "text" && selectedText.value && at > selectedText.value.timeline_start && at < textEnd(selectedText.value) ? s
        : s?.kind === "audio" && selectedAudio.value && inside(selectedAudio.value.clip) ? s
        : current.value && inside(current.value.clip) ? { kind: "clip", id: current.value.clip.id }
        : null;
      if (!target) {
        error.value = clips.value.length ? "Move the playhead inside a clip to split it" : "Nothing to split";
        return;
      }
      const res = await run("split", () =>
        target.kind === "overlay" ? api.overlaySplit(target.id, at)
        : target.kind === "text" ? api.textSplit(target.id, at)
        : target.kind === "audio" ? api.audioClipSplit(target.id, at)
        : api.clipSplit(target.id, at));
      if (!res) return;
      apply(res.project);
      const next: Selection = target.kind === "audio" ? { kind: "audio", id: res.new_id, trackId: target.trackId } : { kind: target.kind as "clip" | "overlay" | "text", id: res.new_id };
      selected.value = stillSelected(res.project, next) ? next : null;
    },
    async deleteClip(id: string) { await edit("delete", () => api.clipDelete(id)); },
    /** ⌫: delete the selection, whatever kind it is. */
    async deleteSelected() {
      const s = selected.value; if (!s) return;
      if (s.kind === "clip") await edit("delete", () => api.clipDelete(s.id));
      else if (s.kind === "overlay") await edit("delete", () => api.overlayDelete(s.id));
      else if (s.kind === "audio") await edit("delete", () => api.audioClipDelete(s.id));
      else if (s.kind === "text") await edit("delete", () => api.textDelete(s.id));
      // the captions track cannot be deleted, only switched off
    },
    async moveClip(id: string, to: number) { await edit("move", () => api.clipMove(id, to)); },
    async setFades(id: string, fi: Ms, fo: Ms) { await edit("fades", () => api.clipSetFades(id, fi, fo)); },
    async setTransition(id: string, t: Transition) { await edit("transition", () => api.clipSetTransition(id, t)); },
    async setVolume(id: string, v: number, muted: boolean) { await edit("volume", () => api.clipSetVolume(id, v, muted)); },
    async renameClip(id: string, name: string) { await edit("rename", () => api.clipRename(id, name)); },
    /** Right-click → split audio from video. Selects the new audio clip. */
    async detachAudio(id: string, trackId: string | null = null) {
      const before = new Set(audioTracks.value.flatMap((t) => t.clips.map((c) => c.id)));
      if (!(await edit("detach", () => api.clipDetachAudio(id, trackId)))) return;
      for (const t of audioTracks.value) for (const c of t.clips) if (!before.has(c.id)) { selected.value = { kind: "audio", id: c.id, trackId: t.id }; return; }
    },
    async setAspect(a: AspectPreset) { await edit("aspect", () => api.setAspect(a)); },
    async setCrop(c: Crop) { await edit("crop", () => api.setCrop(c)); },
    async setVideoMuted(muted: boolean) { await edit("mute", () => api.setVideoMuted(muted)); },
    async setVideoVolume(volume: number) { await edit("volume", () => api.setVideoVolume(volume)); },

    async overlayAdd(path: string, at: Ms, layer = 0) {
      if (await edit("overlay", () => api.overlayAdd(path, at, layer))) {
        const o = overlays.value.find((o) => o.source === path && o.layer === layer && o.timeline_start >= at - 1) ?? overlays.value[overlays.value.length - 1];
        if (o) selected.value = { kind: "overlay", id: o.id };
      }
    },
    async overlayMove(id: string, at: Ms, layer: number) { await edit("move", () => api.overlayMove(id, at, layer)); },
    async overlayLayerAdd() { await edit("layer", () => api.overlayLayerAdd()); },
    async overlayLayerRemove(layer: number) { await edit("layer", () => api.overlayLayerRemove(layer)); },
    async overlayTrim(id: string, s: Ms, e: Ms) { await edit("trim", () => api.overlayTrim(id, s, e)); },
    async overlaySetFades(id: string, fi: Ms, fo: Ms) { await edit("fades", () => api.overlaySetFades(id, fi, fo)); },
    async overlaySetOpacity(id: string, opacity: number) { await edit("opacity", () => api.overlaySetOpacity(id, opacity)); },
    async overlaySetAudio(id: string, volume: number, muted: boolean) { await edit("volume", () => api.overlaySetAudio(id, volume, muted)); },
    async overlayLayerSetAudio(layer: number, muted: boolean, volume: number) { await edit("layer", () => api.overlayLayerSetAudio(layer, muted, volume)); },
    async overlaySetPlacement(id: string, p: Placement) { await edit("placement", () => api.overlaySetPlacement(id, p)); },

    /** + Text: a "Title" at `at` on row `layer`, selected so the Inspector opens on it. */
    async textAdd(at: Ms, text = "Title", layer = 0) {
      const before = new Set(texts.value.map((t) => t.id));
      if (await edit("text", () => api.textAdd(text, at, layer))) {
        const t = texts.value.find((t) => !before.has(t.id));
        if (t) selected.value = { kind: "text", id: t.id };
      }
    },
    async textMove(id: string, at: Ms, layer: number) { await edit("move", () => api.textMove(id, at, layer)); },
    async textLayerAdd() { await edit("layer", () => api.textLayerAdd()); },
    async textLayerRemove(layer: number) { await edit("layer", () => api.textLayerRemove(layer)); },
    async textDelete(id: string) { await edit("delete", () => api.textDelete(id)); },
    async textTrim(id: string, duration: Ms) { await edit("trim", () => api.textTrim(id, duration)); },
    async textSetFades(id: string, fi: Ms, fo: Ms) { await edit("fades", () => api.textSetFades(id, fi, fo)); },
    async textSetPosition(id: string, x: number, y: number) { await edit("position", () => api.textSetPosition(id, x, y)); },
    async textSetStyle(id: string, style: TextStyle) { await edit("style", () => api.textSetStyle(id, style)); },
    loadFonts,
    /** One PNG per title inside V1, drawn at the output size, for `export_start`. */
    textRasters(w: number, h: number): TextRaster[] {
      return texts.value.filter((t) => t.timeline_start < duration.value).map((t) => ({ id: t.id, png: rasterBase64(rasterText(t.style, w, h)) }));
    },

    async audioTrackAdd(label: string) { await edit("track", () => api.audioTrackAdd(label)); },
    async audioTrackUpdate(id: string, label: string, muted: boolean, volume: number) { await edit("track", () => api.audioTrackUpdate(id, label, muted, volume)); },
    async audioTrackRemove(id: string) { await edit("track", () => api.audioTrackRemove(id)); },
    async audioClipAdd(trackId: string, path: string, at: Ms) {
      if (await edit("audio", () => api.audioClipAdd(trackId, path, at))) {
        const t = audioTracks.value.find((t) => t.id === trackId);
        const c = t?.clips.find((c) => c.source === path && c.timeline_start >= at - 1) ?? t?.clips[t.clips.length - 1];
        if (c) selected.value = { kind: "audio", id: c.id, trackId };
      }
    },
    async audioClipMove(id: string, trackId: string, at: Ms) {
      if (await edit("move", () => api.audioClipMove(id, trackId, at)) && selected.value?.kind === "audio" && selected.value.id === id) {
        selected.value = { kind: "audio", id, trackId };
      }
    },
    async audioClipTrim(id: string, s: Ms, e: Ms) { await edit("trim", () => api.audioClipTrim(id, s, e)); },
    async audioClipSet(id: string, volume: number, fi: Ms, fo: Ms, muted: boolean) { await edit("audio", () => api.audioClipSet(id, volume, fi, fo, muted)); },

    /** `extend` (⇧): toggle `sel` in the multi-selection when it shares the primary's row; otherwise it replaces everything. */
    select(sel: string | Selection | null, extend = false) {
      const next = typeof sel === "string" ? { kind: "clip" as const, id: sel } : sel;
      const cur = selected.value;
      if (extend && next && cur && sameRow(cur, next)) {
        if (sameSel(cur, next)) return;
        extraSelected.value = extraSelected.value.some((s) => sameSel(s, next)) ? extraSelected.value.filter((s) => !sameSel(s, next)) : [...extraSelected.value, next];
        return;
      }
      extraSelected.value = [];
      selected.value = next;
    },
    /** ⌘J / right-click → Merge: joins the selected pieces (all one row) back into one clip and selects it. */
    async mergeSelected() {
      const ids = selectedAll.value.map((s) => s.id);
      if (ids.length < 2) return;
      const kind = selected.value!;
      const res = await run("merge", () => api.clipMerge(ids));
      if (!res) return;
      apply(res.project);
      const next: Selection = kind.kind === "audio" ? { kind: "audio", id: res.new_id, trackId: kind.trackId } : { kind: kind.kind as "clip" | "overlay", id: res.new_id };
      selected.value = stillSelected(res.project, next) ? next : null;
      extraSelected.value = [];
    },
    seek,

    // ---- range & pinned loops ----
    range, loopOn, activeLoopId, loops, activeLoop, loopRange, loopsOpen, exportOpen, aiOpen, libraryWidth, loopsHeight, revealRange, textLayers,
    /** Export short on a highlight card: select its span and open the Export dialog on it. */
    exportRange(r: Range) { playing.value = false; setRange(r); exportOpen.value = true; },
    setRange,
    selectLoop,
    togglePlay,
    /** ▶ on a loop / highlight: play `r` on repeat from its start; pressed again while it plays, pause. */
    playRange(r: Range, id: string | null = null) {
      if (playing.value && loopOn.value && sameRange(range.value, r)) { playing.value = false; return; }
      setRange(r);
      if (!range.value) return;
      activeLoopId.value = id;
      loopOn.value = true;
      seek(range.value.start);
      playing.value = true;
    },
    /** Pin `r` (default: the current range) under `name`; the new loop becomes the active one. */
    async loopAdd(name: string, r: Range | null = range.value) {
      if (!r) return;
      const before = new Set(loops.value.map((l) => l.id));
      if (await edit("loop", () => api.loopAdd(name, r.start, r.end))) {
        const l = loops.value.find((l) => !before.has(l.id));
        if (l) { range.value = { start: l.start, end: l.end }; activeLoopId.value = l.id; revealRange.value++; }
      }
    },
    async loopRemove(id: string) { await edit("loop", () => api.loopRemove(id)); },
    /** Export in the drawer: select the loop and open the Export dialog on it. */
    exportLoop(id: string) { playing.value = false; selectLoop(id); exportOpen.value = true; },
  };
});
