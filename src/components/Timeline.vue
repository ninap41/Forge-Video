<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { CAPTIONS_SELECTION, useProjectStore } from "../stores/project";
import ClipBlock from "./ClipBlock.vue";
import FreeBlock from "./FreeBlock.vue";
import TextBlock from "./TextBlock.vue";
import MuteToggle from "./MuteToggle.vue";
import { clipDuration, clipEnd, clipName, mediaKind } from "../types/project";
import type { AudioClip, Clip, OverlayClip, PoolItem, TextClip } from "../types/project";
import { basename, clamp, fmtMs } from "../utils/time";

const store = useProjectStore();
const scroller = ref<HTMLDivElement | null>(null);
const zoom = ref(1); // multiplier over "fit"
const viewW = ref(800);
const PAD = 24;
const MIN_CLIP = 100;
const SNAP_PX = 8;

/** Row heights (px). The label gutter on the left mirrors these exactly. */
const ROW = { ruler: 24, text: 30, overlay: 44, video: 78, caption: 22, audio: 32, gap: 6 } as const;

const fitPxPerMs = computed(() => Math.max(0.005, (viewW.value - PAD * 2) / Math.max(store.duration, 10_000)));
const pxPerMs = computed(() => fitPxPerMs.value * zoom.value);
const contentW = computed(() => Math.max(viewW.value, store.duration * pxPerMs.value + PAD * 2));

const ticks = computed(() => {
  const steps = [100, 250, 500, 1000, 2000, 5000, 10000, 30000, 60000, 300000];
  const step = steps.find((s) => s * pxPerMs.value >= 70) ?? 300000;
  const out = [];
  for (let t = 0; t <= Math.max(store.duration, 10_000); t += step) out.push({ t, x: t * pxPerMs.value });
  return out;
});

// ---- local overrides while dragging (so we don't round-trip Rust per pointer move) ----
type FreeRef = { kind: "overlay"; clip: OverlayClip } | { kind: "audio"; clip: AudioClip; trackId: string };
type Drag =
  | { kind: "trimStart" | "trimEnd"; clip: Clip; x0: number; s0: number; e0: number; s: number; e: number }
  | { kind: "move"; clip: Clip; x0: number; dx: number; moved: boolean }
  | { kind: "freeTrimStart" | "freeTrimEnd"; free: FreeRef; x0: number; s0: number; e0: number; s: number; e: number }
  | { kind: "freeMove"; free: FreeRef; x0: number; dx: number; moved: boolean; trackId?: string; layer?: number }
  | { kind: "textMove"; text: TextClip; x0: number; dx: number; moved: boolean; layer?: number }
  | { kind: "textTrimStart" | "textTrimEnd"; text: TextClip; x0: number; start: number; duration: number }
  | { kind: "scrub" }
  /** Ruler press: a plain click scrubs; once it moves it selects a range from `anchor`. */
  | { kind: "ruler"; x0: number; anchor: number; ranging: boolean }
  | { kind: "rangeStart" | "rangeEnd"; x0: number; other: number };
const drag = ref<Drag | null>(null);

const displayClips = computed(() => {
  const d = drag.value;
  if (!d || d.kind === "scrub" || d.kind === "ruler" || d.kind.startsWith("range") || d.kind.startsWith("free") || d.kind.startsWith("text")) return store.clips;
  if (d.kind === "move") {
    return store.clips.map((c) => (c.id === d.clip.id ? { ...c, timeline_start: c.timeline_start + d.dx } : c));
  }
  // Trim: show the edited clip and ripple the rest locally.
  let cursor = 0;
  return store.clips.map((c) => {
    const cc = c.id === (d as { clip: Clip }).clip.id ? { ...c, source_start: (d as { s: number }).s, source_end: (d as { e: number }).e } : { ...c };
    cc.timeline_start = cursor;
    cursor += clipDuration(cc);
    cursor -= cc.transition_out.type === "None" ? 0 : cc.transition_out.ms;
    return cc;
  });
});

/** Free clips with the in-flight drag applied. Trims on free clips keep the content anchored. */
function withDrag<T extends OverlayClip | AudioClip>(list: T[], kind: "overlay" | "audio"): T[] {
  const d = drag.value;
  if (!d || !d.kind.startsWith("free") || (d as { free: FreeRef }).free.kind !== kind) return list;
  const f = (d as { free: FreeRef }).free;
  return list.map((c) => {
    if (c.id !== f.clip.id) return c;
    if (d.kind === "freeMove") return { ...c, timeline_start: Math.max(0, c.timeline_start + d.dx) };
    const dd = d as { s: number; e: number; s0: number };
    const shift = c.media.is_still ? 0 : dd.s - dd.s0;
    return { ...c, source_start: dd.s, source_end: dd.e, timeline_start: Math.max(0, c.timeline_start + shift) };
  });
}
/** Titles with the in-flight drag applied (a cross-row move is drawn on its target row). */
const displayTexts = computed(() => {
  const d = drag.value;
  if (!d || !d.kind.startsWith("text")) return store.texts;
  const t = (d as { text: TextClip }).text;
  return store.texts.map((x) => {
    if (x.id !== t.id) return x;
    if (d.kind === "textMove") return { ...x, timeline_start: Math.max(0, x.timeline_start + d.dx), layer: d.layer ?? x.layer };
    const dd = d as { start: number; duration: number };
    return { ...x, timeline_start: dd.start, duration: dd.duration };
  });
});
/** One row per text layer (T1 = 0). */
const displayTextLayers = computed(() => Array.from({ length: store.textLayers }, (_, layer) => ({ layer, clips: displayTexts.value.filter((t) => t.layer === layer) })));
/** One row per overlay layer, with an in-flight cross-layer drag drawn on its target row. */
const displayLayers = computed(() => {
  const d = drag.value;
  const all = withDrag(store.overlays, "overlay");
  return Array.from({ length: store.overlayLayers }, (_, layer) => {
    let clips = all.filter((o) => o.layer === layer);
    if (d?.kind === "freeMove" && d.free.kind === "overlay" && d.layer !== undefined && d.layer !== d.free.clip.layer) {
      if (layer === d.free.clip.layer) clips = clips.filter((o) => o.id !== d.free.clip.id);
      if (layer === d.layer) clips = [...clips, { ...d.free.clip, layer, timeline_start: Math.max(0, d.free.clip.timeline_start + d.dx) }];
    }
    return { layer, clips };
  });
});
const displayTracks = computed(() => store.audioTracks.map((t) => {
  const d = drag.value;
  let clips = withDrag(t.clips, "audio");
  // A clip being dragged onto another track is drawn there instead.
  if (d?.kind === "freeMove" && d.free.kind === "audio" && d.trackId && d.trackId !== d.free.trackId) {
    if (t.id === d.free.trackId) clips = clips.filter((c) => c.id !== d.free.clip.id);
    if (t.id === d.trackId) clips = [...clips, { ...d.free.clip, timeline_start: Math.max(0, d.free.clip.timeline_start + d.dx) }];
  }
  return { track: t, clips };
}));

function xToMs(clientX: number) {
  const el = scroller.value!; const r = el.getBoundingClientRect();
  return (clientX - r.left + el.scrollLeft - PAD) / pxPerMs.value;
}

/** Snap a free-clip start to V1 clip edges and the playhead when within a few pixels. */
function snapFree(t: number, len: number) {
  const targets = [0, store.playhead, ...store.clips.flatMap((c) => [c.timeline_start, clipEnd(c)])];
  const tol = SNAP_PX / pxPerMs.value;
  for (const s of targets) {
    if (Math.abs(t - s) <= tol) return s;
    if (Math.abs(t + len - s) <= tol) return Math.max(0, s - len);
  }
  return Math.max(0, t);
}

function startTrim(c: Clip, e: PointerEvent, kind: "trimStart" | "trimEnd") {
  drag.value = { kind, clip: c, x0: e.clientX, s0: c.source_start, e0: c.source_end, s: c.source_start, e: c.source_end };
  capture(e);
}
function startMove(c: Clip, e: PointerEvent) {
  drag.value = { kind: "move", clip: c, x0: e.clientX, dx: 0, moved: false };
  capture(e);
}
function startFreeTrim(free: FreeRef, e: PointerEvent, kind: "freeTrimStart" | "freeTrimEnd") {
  const c = free.clip;
  drag.value = { kind, free, x0: e.clientX, s0: c.source_start, e0: c.source_end, s: c.source_start, e: c.source_end };
  capture(e);
}
function startFreeMove(free: FreeRef, e: PointerEvent) {
  drag.value = { kind: "freeMove", free, x0: e.clientX, dx: 0, moved: false, trackId: free.kind === "audio" ? free.trackId : undefined, layer: free.kind === "overlay" ? free.clip.layer : undefined };
  capture(e);
}
function startTextMove(text: TextClip, e: PointerEvent) {
  drag.value = { kind: "textMove", text, x0: e.clientX, dx: 0, moved: false, layer: text.layer };
  capture(e);
}
function startTextTrim(text: TextClip, e: PointerEvent, kind: "textTrimStart" | "textTrimEnd") {
  drag.value = { kind, text, x0: e.clientX, start: text.timeline_start, duration: text.duration };
  capture(e);
}
function startScrub(e: PointerEvent) {
  drag.value = { kind: "scrub" };
  store.playing = false;
  store.seek(xToMs(e.clientX));
  capture(e);
}
/** The ruler: click to scrub, drag to select a range (GarageBand's cycle region). */
function startRuler(e: PointerEvent) {
  const anchor = xToMs(e.clientX);
  drag.value = { kind: "ruler", x0: e.clientX, anchor, ranging: false };
  store.playing = false;
  store.seek(anchor);
  capture(e);
}
function startRangeHandle(e: PointerEvent, kind: "rangeStart" | "rangeEnd") {
  const r = store.range; if (!r) return;
  drag.value = { kind, x0: e.clientX, other: kind === "rangeStart" ? r.end : r.start };
  capture(e);
}
/** Range edges snap to clip boundaries and the playhead like free clips do. */
function snapPoint(t: number) {
  const targets = [0, store.duration, store.playhead, ...store.clips.flatMap((c) => [c.timeline_start, clipEnd(c)])];
  const tol = SNAP_PX / pxPerMs.value;
  const hit = targets.find((s) => Math.abs(t - s) <= tol);
  return clamp(hit ?? t, 0, store.duration);
}
function capture(e: PointerEvent) { (scroller.value as HTMLElement).setPointerCapture(e.pointerId); }

function rowAt(clientY: number): string | null {
  const left = scroller.value?.getBoundingClientRect().left ?? 0;
  const el = document.elementFromPoint(left + 5, clientY)?.closest<HTMLElement>("[data-row]");
  return el?.dataset.row ?? null;
}

function onMove(e: PointerEvent) {
  const d = drag.value; if (!d) return;
  if (d.kind === "scrub") { store.seek(xToMs(e.clientX)); return; }
  if (d.kind === "ruler") {
    if (!d.ranging && Math.abs(e.clientX - d.x0) <= 4) { store.seek(xToMs(e.clientX)); return; }
    d.ranging = true;
    const t = snapPoint(xToMs(e.clientX));
    store.setRange({ start: Math.min(d.anchor, t), end: Math.max(d.anchor, t) });
    return;
  }
  if (d.kind === "rangeStart") { const t = snapPoint(xToMs(e.clientX)); store.setRange({ start: Math.min(t, d.other - MIN_CLIP), end: d.other }); return; }
  if (d.kind === "rangeEnd") { const t = snapPoint(xToMs(e.clientX)); store.setRange({ start: d.other, end: Math.max(t, d.other + MIN_CLIP) }); return; }
  const dms = (e.clientX - d.x0) / pxPerMs.value;
  if (d.kind === "textMove") {
    d.dx = dms; if (Math.abs(e.clientX - d.x0) > 4) d.moved = true;
    const row = rowAt(e.clientY);
    if (row?.startsWith("text:")) d.layer = Number(row.slice(5));
    return;
  }
  if (d.kind === "textTrimEnd") { d.duration = Math.max(MIN_CLIP, Math.round((d.text.duration + dms) / 10) * 10); return; }
  if (d.kind === "textTrimStart") {
    const start = clamp(Math.round((d.text.timeline_start + dms) / 10) * 10, 0, d.text.timeline_start + d.text.duration - MIN_CLIP);
    d.start = start; d.duration = d.text.timeline_start + d.text.duration - start; return;
  }
  if (d.kind === "move" || d.kind === "freeMove") {
    d.dx = dms; if (Math.abs(e.clientX - d.x0) > 4) d.moved = true;
    if (d.kind === "freeMove") {
      const row = rowAt(e.clientY);
      if (d.free.kind === "audio" && row?.startsWith("audio:")) d.trackId = row.slice(6);
      if (d.free.kind === "overlay" && row?.startsWith("overlay:")) d.layer = Number(row.slice(8));
    }
    return;
  }
  const clip = d.kind.startsWith("free") ? (d as { free: FreeRef }).free.clip : (d as { clip: Clip }).clip;
  const fpsNum = clip.media.fps.num, fpsDen = clip.media.fps.den;
  const frame = fpsNum > 0 ? 1000 / (fpsNum / Math.max(1, fpsDen)) : 10;
  const snap = (v: number) => Math.round(v / frame) * frame;
  const maxEnd = clip.media.is_still ? Number.MAX_SAFE_INTEGER : clip.media.duration_ms;
  const dd = d as { s: number; e: number; s0: number; e0: number };
  if (d.kind === "trimStart" || d.kind === "freeTrimStart") dd.s = clip.media.is_still ? 0 : clamp(snap(dd.s0 + dms), 0, dd.e0 - MIN_CLIP);
  else dd.e = clamp(snap(dd.e0 + dms), dd.s0 + MIN_CLIP, maxEnd);
}
async function onUp() {
  const d = drag.value; drag.value = null;
  if (!d || d.kind === "scrub" || d.kind === "rangeStart" || d.kind === "rangeEnd") return;
  if (d.kind === "ruler") {
    // A plain click outside the yellow band clears it; inside, it only scrubbed.
    const r = store.range;
    if (!d.ranging && r && (d.anchor < r.start || d.anchor > r.end)) store.setRange(null);
    return;
  }
  if (d.kind === "move") {
    if (!d.moved) return;
    const centre = d.clip.timeline_start + d.dx + clipDuration(d.clip) / 2;
    let to = 0;
    for (const c of store.clips) if (c.id !== d.clip.id && c.timeline_start + clipDuration(c) / 2 < centre) to++;
    const from = store.clips.findIndex((c) => c.id === d.clip.id);
    if (to !== from) await store.moveClip(d.clip.id, to);
    return;
  }
  if (d.kind === "textMove") {
    if (!d.moved) return;
    await store.textMove(d.text.id, Math.round(snapFree(d.text.timeline_start + d.dx, d.text.duration)), d.layer ?? d.text.layer);
    return;
  }
  if (d.kind === "textTrimStart" || d.kind === "textTrimEnd") {
    if (d.start === d.text.timeline_start && d.duration === d.text.duration) return;
    await store.textTrim(d.text.id, d.duration);
    if (d.start !== d.text.timeline_start) await store.textMove(d.text.id, d.start, d.text.layer);
    return;
  }
  if (d.kind === "freeMove") {
    if (!d.moved) return;
    const at = Math.round(snapFree(d.free.clip.timeline_start + d.dx, clipDuration(d.free.clip)));
    if (d.free.kind === "overlay") await store.overlayMove(d.free.clip.id, at, d.layer ?? d.free.clip.layer);
    else await store.audioClipMove(d.free.clip.id, d.trackId ?? d.free.trackId, at);
    return;
  }
  if (d.kind === "trimStart" || d.kind === "trimEnd") {
    if (d.s !== d.s0 || d.e !== d.e0) await store.trim(d.clip.id, Math.round(d.s), Math.round(d.e));
    return;
  }
  if (d.kind !== "freeTrimStart" && d.kind !== "freeTrimEnd") return;
  if (d.s === d.s0 && d.e === d.e0) return;
  const s = Math.round(d.s), e = Math.round(d.e);
  const f = d.free;
  const shifted = Math.max(0, f.clip.timeline_start + (s - d.s0));
  if (f.kind === "overlay") {
    await store.overlayTrim(f.clip.id, s, e);
    if (!f.clip.media.is_still && s !== d.s0) await store.overlayMove(f.clip.id, shifted, f.clip.layer);
  } else {
    await store.audioClipTrim(f.clip.id, s, e);
    if (s !== d.s0) await store.audioClipMove(f.clip.id, f.trackId, shifted);
  }
}

// ---- drops from the media pool (pointer-driven; see MediaPool.vue) ----
function poolAccepts(row: string, item: PoolItem) {
  const k = mediaKind(item.media);
  if (row === "video") return k !== "Audio";
  if (row.startsWith("overlay:")) return k !== "Audio";
  return row.startsWith("audio:") && item.media.has_audio;
}
async function onPoolDrop(row: string, e: CustomEvent<{ item: PoolItem; clientX: number }>) {
  const { item, clientX } = e.detail;
  if (!poolAccepts(row, item)) {
    const k = mediaKind(item.media);
    if (row.startsWith("text")) store.notify("Text rows only hold titles — press + Text to add one");
    else if (k === "Audio") store.notify("Audio goes on an audio track — drop it on a ♪ row");
    else if (item.media.has_audio) store.notify("Video and images go on V1 or a video track");
    else store.notify(`${basename(item.path)} has no audio — video and images go on V1 or a video track`);
    return;
  }
  const at = Math.round(Math.max(0, xToMs(clientX)));
  if (row === "video") {
    let to = 0;
    for (const c of store.clips) if (c.timeline_start + clipDuration(c) / 2 < at) to++;
    await store.insertClip(item.path, to);
  } else if (row.startsWith("overlay:")) {
    await store.overlayAdd(item.path, at, Number(row.slice(8)));
  } else {
    await store.audioClipAdd(row.slice(6), item.path, at);
  }
}
const dropHint = computed(() => {
  const d = store.poolDrag; if (!d) return null;
  return { video: poolAccepts("video", d.item), overlay: poolAccepts("overlay:0", d.item), audio: d.item.media.has_audio };
});

// Keep playhead visible during playback.
watch(() => store.playhead, (t) => {
  const el = scroller.value; if (!el || !store.playing) return;
  const x = t * pxPerMs.value + PAD;
  if (x < el.scrollLeft + 40 || x > el.scrollLeft + el.clientWidth - 40) el.scrollLeft = x - el.clientWidth / 3;
});

/** A loop was pinned or picked: bring its start into view (a little left of it) unless it is already visible. */
watch(() => store.revealRange, () => {
  const el = scroller.value, r = store.range; if (!el || !r) return;
  const x0 = r.start * pxPerMs.value + PAD, x1 = r.end * pxPerMs.value + PAD;
  if (x0 >= el.scrollLeft && x1 <= el.scrollLeft + el.clientWidth) return;
  el.scrollLeft = Math.max(0, x0 - 40);
});

const ro = new ResizeObserver(([e]) => { viewW.value = e.contentRect.width; });
onMounted(() => { if (scroller.value) ro.observe(scroller.value); });

// ---- right-click menu on any clip; `renaming` swaps the items for a name field ----
type MenuClip = { kind: "clip"; clip: Clip } | { kind: "overlay"; clip: OverlayClip } | { kind: "audio"; clip: AudioClip } | { kind: "text"; clip: TextClip };
const menu = ref<(MenuClip & { x: number; y: number; renaming: boolean }) | null>(null);
function openMenu(target: MenuClip, e: MouseEvent) { menu.value = { ...target, x: e.clientX, y: e.clientY, renaming: false }; }
const canDetach = (c: Clip) => c.media.has_audio && !c.media.is_still;
async function deleteFromMenu() {
  const m = menu.value; menu.value = null;
  if (m?.kind === "text") await store.textDelete(m.clip.id);
}
async function detachAudio() {
  const m = menu.value; menu.value = null;
  if (m?.kind === "clip" && canDetach(m.clip)) await store.detachAudio(m.clip.id);
}
async function commitName(e: Event) {
  const m = menu.value; if (!m?.renaming) return; // Escape or an outside click already closed it
  menu.value = null;
  const name = (e.target as HTMLInputElement).value.trim();
  // Typing the file name (or a title's first line) back, or clearing the field, drops the custom name.
  const next = name === (m.kind === "text" ? clipName({ style: m.clip.style }) : basename(m.clip.source)) ? "" : name;
  if (next !== (m.clip.name ?? "")) await store.renameClip(m.clip.id, next);
}
const vFocus = { mounted: (el: HTMLInputElement) => { el.focus(); el.select(); } };

// ---- "+ Track": one button, every track type ----
const TRACK_TYPES = [
  { label: "Video", hint: "Silent row above V1, composited on top" },
  { label: "Audio", hint: "General audio track" },
  { label: "Music", hint: "Audio track for music beds" },
  { label: "Narration", hint: "Audio track for voice-over" },
  { label: "SFX", hint: "Audio track for sound effects" },
  { label: "Text", hint: "Another row of titles, drawn above the rows below it" },
] as const;
const addMenu = ref<{ x: number; y: number } | null>(null);
function toggleAddMenu(e: MouseEvent) {
  const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
  addMenu.value = addMenu.value ? null : { x: r.left, y: r.bottom + 2 };
}
async function addTrack(label: (typeof TRACK_TYPES)[number]["label"]) {
  addMenu.value = null;
  if (label === "Video") await store.overlayLayerAdd();
  else if (label === "Text") await store.textLayerAdd();
  else await store.audioTrackAdd(label);
}

function onWindowDown(e: Event) {
  const t = e.target as HTMLElement;
  if (menu.value && !t.closest("[data-testid=clip-menu]")) menu.value = null;
  if (addMenu.value && !t.closest("[data-testid=add-track-menu], [data-testid=add-track]")) addMenu.value = null;
}
function onWindowKey(e: KeyboardEvent) {
  if (e.key !== "Escape") return;
  if (menu.value || addMenu.value) { menu.value = null; addMenu.value = null; return; }
  const tag = (e.target as HTMLElement).tagName;
  if (tag !== "INPUT" && tag !== "TEXTAREA" && !store.exportOpen) store.setRange(null);
}
onMounted(() => { window.addEventListener("pointerdown", onWindowDown); window.addEventListener("keydown", onWindowKey); });
onBeforeUnmount(() => { window.removeEventListener("pointerdown", onWindowDown); window.removeEventListener("keydown", onWindowKey); });

// ---- audio track gutter ----
const editingLabel = ref<string | null>(null);
/** A gutter fader: 0–100 % of the track's clips. */
const pct = (v: number) => `${Math.round(v * 100)} %`;
function commitLabel(trackId: string, e: Event, muted: boolean, volume: number) {
  editingLabel.value = null;
  void store.audioTrackUpdate(trackId, (e.target as HTMLInputElement).value, muted, volume);
}
/** Text rows sit at the very top: titles composite above every overlay layer, higher rows on top. */
const textTop = ROW.ruler + ROW.gap;
const textsH = computed(() => store.textLayers * (ROW.text + ROW.gap));
const overlaysTop = computed(() => textTop + textsH.value);
const overlaysH = computed(() => store.overlayLayers * (ROW.overlay + ROW.gap));
const videoTop = computed(() => overlaysTop.value + overlaysH.value);
/** Captions sit right under V1. The row only exists once something has been transcribed. */
const captionTop = computed(() => videoTop.value + ROW.video + ROW.gap);
const hasCaptions = computed(() => store.cues.length > 0);
const audioTop = computed(() => captionTop.value + (hasCaptions.value ? ROW.caption + ROW.gap : 0));
const editingCue = ref<string | null>(null);
function commitCue(id: string, before: string, e: Event) {
  if (editingCue.value !== id) return; // Escape already closed it
  editingCue.value = null;
  const text = (e.target as HTMLInputElement).value.trim();
  if (text !== before) void store.setCueText(id, text);
}
const tracksHeight = computed(() => audioTop.value + store.audioTracks.length * (ROW.audio + ROW.gap) + 24);
const isSelected = (kind: "clip" | "overlay" | "audio" | "text", id: string) => store.selectedAll.some((s) => s.kind === kind && s.id === id);
/** Merge is offered when the right-clicked clip is one of two or more selected pieces on its row. */
const mergeCount = computed(() => (menu.value && menu.value.kind !== "text" && isSelected(menu.value.kind, menu.value.clip.id) ? store.selectedAll.length : 0));
async function mergeSelected() { menu.value = null; await store.mergeSelected(); }
/** Pixel box of the yellow range band. */
const rangeBox = computed(() => (store.range ? { left: store.range.start * pxPerMs.value + PAD, width: (store.range.end - store.range.start) * pxPerMs.value } : null));
const rowClass = (ok: boolean | undefined) => (ok === undefined ? "" : ok ? "ring-1 ring-inset ring-accent/60 bg-accent/5" : "opacity-40");
</script>

<template>
  <div class="flex flex-col h-full bg-panel border-t border-line">
    <div class="flex items-center gap-3 px-3 h-8 shrink-0 border-b border-line text-xs text-muted">
      <span class="font-mono text-fg w-20">{{ fmtMs(store.playhead) }}</span>
      <span>/ {{ fmtMs(store.duration) }}</span>
      <span class="ml-2">{{ store.clips.length }} clip{{ store.clips.length === 1 ? '' : 's' }}</span>
      <template v-if="store.range">
        <span class="ml-2 font-mono text-yellow-400" data-testid="range-label">{{ fmtMs(store.range.start, false) }}–{{ fmtMs(store.range.end, false) }}</span>
        <button
          data-testid="loop-toggle" class="rounded border px-1.5 py-0.5" :class="store.loopOn ? 'border-yellow-400 text-yellow-400 bg-yellow-400/10' : 'border-line hover:text-fg'"
          :aria-pressed="store.loopOn" title="Loop the selected range (L)" @click="store.loopOn = !store.loopOn"
        >⟳ Loop</button>
        <button data-testid="pin-range" class="rounded border border-line px-1.5 py-0.5 hover:text-fg" title="Keep this range in Pinned loops" @click="store.loopAdd(store.activeLoop?.name ?? '')">Pin</button>
        <button class="hover:text-fg" title="Clear the range (Esc)" aria-label="Clear range" @click="store.setRange(null)">✕</button>
      </template>
      <span class="flex-1" />
      <span>Zoom</span>
      <input type="range" min="1" max="20" step="0.25" v-model.number="zoom" class="w-32" />
      <button class="hover:text-fg" @click="zoom = 1">Fit</button>
    </div>

    <div class="flex flex-1 min-h-0 overflow-y-auto border-b border-line" data-testid="timeline-tracks">
     <div class="flex shrink-0 w-full" :style="{ height: tracksHeight + 'px' }">
      <!-- label gutter, mirrors the row heights -->
      <div class="w-24 shrink-0 border-r border-line text-[10px] text-muted select-none">
        <div class="flex items-end justify-end px-1" :style="{ height: ROW.ruler + ROW.gap + 'px' }">
          <button data-testid="add-track" class="hover:text-fg" title="Add a video or audio track" aria-haspopup="menu" :aria-expanded="!!addMenu" @click="toggleAddMenu">+ Track</button>
        </div>
        <div
          v-for="{ layer } in [...displayTextLayers].reverse()" :key="'t' + layer" class="px-2 flex items-center gap-1" data-testid="text-gutter"
          :style="{ height: ROW.text + 'px', marginBottom: ROW.gap + 'px' }"
        >
          <span class="flex-1 truncate">T{{ layer + 1 }} · text</span>
          <button data-testid="add-text" class="hover:text-fg" :title="`Add a title at the playhead on T${layer + 1}`" @click="store.textAdd(store.playhead, 'Title', layer)">+ Text</button>
          <button v-if="store.textLayers > 1" class="w-4 text-center hover:text-danger" :title="`Remove text row T${layer + 1}`" @click="store.textLayerRemove(layer)">✕</button>
        </div>
        <div
          v-for="{ layer } in [...displayLayers].reverse()" :key="layer" class="px-2 flex flex-col justify-center gap-1" :style="{ height: ROW.overlay + 'px', marginBottom: ROW.gap + 'px' }"
        >
          <div class="flex items-center gap-1">
            <span class="flex-1 truncate">V{{ layer + 2 }} · video</span>
            <MuteToggle :muted="store.layerAudio(layer).muted" :label="`V${layer + 2} audio`" :size="14" @toggle="store.overlayLayerSetAudio(layer, !store.layerAudio(layer).muted, store.layerAudio(layer).volume)" />
            <button v-if="store.overlayLayers > 1" class="w-4 text-center hover:text-danger" :title="`Remove track V${layer + 2}`" @click="store.overlayLayerRemove(layer)">✕</button>
          </div>
          <input
            type="range" min="0" max="1" step="0.05" class="fader" :data-testid="'fader-v' + (layer + 2)" :aria-label="`V${layer + 2} volume`"
            :value="store.layerAudio(layer).volume" :title="`V${layer + 2} volume ${pct(store.layerAudio(layer).volume)}`"
            @input="store.overlayLayerSetAudio(layer, store.layerAudio(layer).muted, Number(($event.target as HTMLInputElement).value))"
          />
        </div>
        <div class="px-2 flex flex-col justify-center gap-1" :style="{ height: ROW.video + 'px', marginBottom: ROW.gap + 'px' }">
          <div class="flex items-center gap-1">
            <span class="flex-1 truncate">V1 · video</span>
            <MuteToggle :muted="!!store.project?.video_muted" label="video audio" :size="14" @toggle="store.setVideoMuted(!store.project?.video_muted)" />
          </div>
          <input
            type="range" min="0" max="1" step="0.05" class="fader" data-testid="fader-v1" aria-label="V1 volume"
            :value="store.project?.video_volume ?? 1" :title="`V1 volume ${pct(store.project?.video_volume ?? 1)}`"
            @input="store.setVideoVolume(Number(($event.target as HTMLInputElement).value))"
          />
        </div>
        <div v-if="hasCaptions" class="px-2 flex items-center" data-testid="caption-gutter" :style="{ height: ROW.caption + 'px', marginBottom: ROW.gap + 'px' }">
          <button
            class="flex-1 text-left truncate hover:text-fg" :class="store.selectedCaptions ? 'text-accent' : ''" :title="store.captionsEnabled ? 'Select the captions track' : 'Captions are off: no preview caption, no .srt on export'"
            @click="store.select(CAPTIONS_SELECTION)"
          >CC · captions{{ store.captionsEnabled ? '' : ' · off' }}</button>
        </div>
        <div v-for="{ track } in displayTracks" :key="track.id" class="px-1 flex flex-col justify-center gap-0.5" :style="{ height: ROW.audio + 'px', marginBottom: ROW.gap + 'px' }">
          <div class="flex items-center gap-1">
            <input
              v-if="editingLabel === track.id" :value="track.label" list="track-presets" class="w-12 bg-panel-2 border border-line rounded px-1 text-fg" autofocus
              @change="commitLabel(track.id, $event, track.muted, track.volume)" @blur="commitLabel(track.id, $event, track.muted, track.volume)" @keydown.enter="($event.target as HTMLInputElement).blur()"
            />
            <button v-else class="flex-1 text-left truncate hover:text-fg" :title="'Rename ' + track.label" @click="editingLabel = track.id">♪ {{ track.label }}</button>
            <MuteToggle :muted="track.muted" label="track" :size="14" @toggle="store.audioTrackUpdate(track.id, track.label, !track.muted, track.volume)" />
            <button class="w-4 text-center hover:text-danger" title="Remove track" @click="store.audioTrackRemove(track.id)">✕</button>
          </div>
          <input
            type="range" min="0" max="1" step="0.05" class="fader" :data-testid="'fader-' + track.id" :aria-label="`${track.label} volume`"
            :value="track.volume" :title="`${track.label} volume ${pct(track.volume)}`"
            @input="store.audioTrackUpdate(track.id, track.label, track.muted, Number(($event.target as HTMLInputElement).value))"
          />
        </div>
        <datalist id="track-presets"><option value="Music" /><option value="SFX" /><option value="Narration" /><option value="Other" /></datalist>
      </div>

      <div
        ref="scroller" class="relative flex-1 overflow-x-auto overflow-y-hidden" style="cursor: default"
        @pointermove="onMove" @pointerup="onUp" @pointercancel="onUp"
      >
        <div class="relative h-full" :style="{ width: contentW + 'px' }">
          <!-- ruler -->
          <div class="absolute inset-x-0 top-0 border-b border-line cursor-text" :style="{ height: ROW.ruler + 'px' }" @pointerdown="startRuler">
            <div v-for="k in ticks" :key="k.t" class="absolute top-0 h-full border-l border-line/80 text-[10px] text-muted pl-1 pt-1" :style="{ left: k.x + PAD + 'px' }">{{ fmtMs(k.t, false) }}</div>
            <!-- range: a solid strip on the ruler with a handle at each end -->
            <template v-if="rangeBox">
              <div class="absolute bottom-0 h-1.5 bg-yellow-400 pointer-events-none" :style="{ left: rangeBox.left + 'px', width: rangeBox.width + 'px' }" />
              <div data-testid="range-handle-start" class="absolute top-0 h-full w-2 -ml-1 cursor-ew-resize" :style="{ left: rangeBox.left + 'px' }" @pointerdown.stop="startRangeHandle($event, 'rangeStart')" />
              <div data-testid="range-handle-end" class="absolute top-0 h-full w-2 -ml-1 cursor-ew-resize" :style="{ left: rangeBox.left + rangeBox.width + 'px' }" @pointerdown.stop="startRangeHandle($event, 'rangeEnd')" />
            </template>
          </div>

          <!-- text rows: titles burned in above everything, highest row on top -->
          <div
            v-for="{ layer, clips } in displayTextLayers" :key="'text' + layer"
            class="absolute inset-x-0 rounded-sm" :data-row="'text:' + layer" :class="rowClass(dropHint ? false : undefined)"
            :style="{ top: textTop + (store.textLayers - 1 - layer) * (ROW.text + ROW.gap) + 'px', height: ROW.text + 'px', paddingLeft: PAD + 'px' }"
            @pointerdown="startScrub" @pooldrop="onPoolDrop('text:' + layer, $event as CustomEvent)"
          >
            <div class="relative h-full">
              <TextBlock
                v-for="t in clips" :key="t.id" :clip="t" :px-per-ms="pxPerMs" :selected="isSelected('text', t.id)" :dimmed="t.timeline_start >= store.duration"
                @select="store.select({ kind: 'text', id: t.id }, $event)"
                @trim-start="startTextTrim(t, $event, 'textTrimStart')" @trim-end="startTextTrim(t, $event, 'textTrimEnd')"
                @drag-start="startTextMove(t, $event)" @context-menu="openMenu({ kind: 'text', clip: t }, $event)"
              />
              <div v-if="!clips.length && layer === 0" class="text-[11px] text-muted/60 pt-2 pl-1 pointer-events-none">+ Text adds a title at the playhead · any font, colour and backdrop in the Inspector</div>
            </div>
          </div>

          <!-- overlay layers: highest on top, V2 just above V1 -->
          <div
            v-for="{ layer, clips } in displayLayers" :key="layer"
            class="absolute inset-x-0 rounded-sm" :data-row="'overlay:' + layer" :class="rowClass(dropHint?.overlay)"
            :style="{ top: overlaysTop + (store.overlayLayers - 1 - layer) * (ROW.overlay + ROW.gap) + 'px', height: ROW.overlay + 'px', paddingLeft: PAD + 'px' }"
            @pointerdown="startScrub" @pooldrop="onPoolDrop('overlay:' + layer, $event as CustomEvent)"
          >
            <div class="relative h-full">
              <FreeBlock
                v-for="o in clips" :key="o.id" :clip="o" kind="overlay" :px-per-ms="pxPerMs" :selected="isSelected('overlay', o.id)"
                :thumbs="store.thumbs[o.source]" :dimmed="o.timeline_start >= store.duration"
                @select="store.select({ kind: 'overlay', id: o.id }, $event)"
                @trim-start="startFreeTrim({ kind: 'overlay', clip: o }, $event, 'freeTrimStart')" @trim-end="startFreeTrim({ kind: 'overlay', clip: o }, $event, 'freeTrimEnd')"
                @drag-start="startFreeMove({ kind: 'overlay', clip: o }, $event)" @context-menu="openMenu({ kind: 'overlay', clip: o }, $event)"
              />
              <div v-if="!clips.length" class="text-[11px] text-muted/60 pt-3 pl-1 pointer-events-none">Drop video or a PNG here · composites over V1 (logos, lower-thirds, B-roll)</div>
            </div>
          </div>

          <!-- V1 video track -->
          <div
            class="absolute inset-x-0 rounded-sm" data-row="video" :class="rowClass(dropHint?.video)"
            :style="{ top: videoTop + 'px', height: ROW.video + 'px', paddingLeft: PAD + 'px' }"
            @pointerdown="startScrub" @pooldrop="onPoolDrop('video', $event as CustomEvent)"
          >
            <div class="relative h-full">
              <ClipBlock
                v-for="(c, i) in displayClips" :key="c.id" :clip="c" :index="i" :px-per-ms="pxPerMs"
                :selected="isSelected('clip', c.id)" :thumbs="store.thumbs[c.source]" :peaks="store.project?.video_muted ? undefined : store.waveforms[c.source]"
                @select="store.select(c.id, $event)" @trim-start="startTrim(c, $event, 'trimStart')" @trim-end="startTrim(c, $event, 'trimEnd')" @drag-start="startMove(c, $event)"
                @context-menu="openMenu({ kind: 'clip', clip: c }, $event)"
              />
            </div>
          </div>

          <!-- captions: derived from the transcript, so they follow every edit to V1 -->
          <div
            v-if="hasCaptions" class="absolute inset-x-0 rounded-sm" data-row="caption"
            :class="[store.selectedCaptions ? 'ring-1 ring-accent/60' : '', store.captionsEnabled ? '' : 'opacity-50']"
            :style="{ top: captionTop + 'px', height: ROW.caption + 'px', paddingLeft: PAD + 'px' }" @pointerdown="store.select(CAPTIONS_SELECTION); startScrub($event)"
          >
            <div class="relative h-full">
              <div
                v-for="(c, i) in store.cues" :key="i" data-testid="cue" class="absolute top-0 h-full rounded-sm border border-line bg-panel-2 text-[10px] leading-[20px] text-fg/90 px-1 truncate"
                :class="store.currentCue?.id === c.id && store.currentCue.start === c.start ? 'border-accent/70' : ''"
                :style="{ left: c.start * pxPerMs + 'px', width: Math.max(1, (c.end - c.start) * pxPerMs - 1) + 'px' }" :title="c.text || 'Double-click to edit'"
                @pointerdown.stop="store.select(CAPTIONS_SELECTION); store.playing = false; store.seek(xToMs($event.clientX))" @dblclick="editingCue = c.id"
              >
                <input
                  v-if="editingCue === c.id" v-focus data-testid="cue-text" aria-label="Caption text" :value="c.text" class="absolute inset-y-0 left-0 min-w-56 w-full bg-panel border border-accent rounded-sm px-1 text-fg z-30"
                  @pointerdown.stop @keydown.stop @keydown.enter="commitCue(c.id, c.text, $event)" @keydown.escape="editingCue = null" @blur="commitCue(c.id, c.text, $event)"
                />
                <template v-else>{{ c.text }}</template>
              </div>
            </div>
          </div>

          <!-- audio tracks -->
          <div
            v-for="({ track, clips }, ti) in displayTracks" :key="track.id"
            class="absolute inset-x-0 rounded-sm" :data-row="'audio:' + track.id" :class="[rowClass(dropHint?.audio), track.muted ? 'opacity-60' : '']"
            :style="{ top: audioTop + ti * (ROW.audio + ROW.gap) + 'px', height: ROW.audio + 'px', paddingLeft: PAD + 'px' }"
            @pointerdown="startScrub" @pooldrop="onPoolDrop('audio:' + track.id, $event as CustomEvent)"
          >
            <div class="relative h-full">
              <FreeBlock
                v-for="c in clips" :key="c.id" :clip="c" kind="audio" :px-per-ms="pxPerMs" :selected="isSelected('audio', c.id)"
                :peaks="store.waveforms[c.source]" :dimmed="c.timeline_start >= store.duration"
                @select="store.select({ kind: 'audio', id: c.id, trackId: track.id }, $event)"
                @trim-start="startFreeTrim({ kind: 'audio', clip: c, trackId: track.id }, $event, 'freeTrimStart')" @trim-end="startFreeTrim({ kind: 'audio', clip: c, trackId: track.id }, $event, 'freeTrimEnd')"
                @drag-start="startFreeMove({ kind: 'audio', clip: c, trackId: track.id }, $event)" @context-menu="openMenu({ kind: 'audio', clip: c }, $event)"
              />
            </div>
          </div>
          <div v-if="!displayTracks.length" class="absolute inset-x-0 text-[11px] text-muted/60 pl-7 pt-1 pointer-events-none" :style="{ top: audioTop + 'px' }">
            No audio tracks · click “+ Track”, then drop audio from the pool
          </div>

          <!-- highlights suggested by AI mode (shown while the AI panel is open) -->
          <div
            v-for="h in store.aiOpen ? store.highlights : []" :key="h.id" data-testid="highlight-band" class="absolute bottom-0 bg-accent/10 border-x border-accent/50 pointer-events-none z-10"
            :style="{ top: ROW.ruler + 'px', left: h.start * pxPerMs + PAD + 'px', width: (h.end - h.start) * pxPerMs + 'px' }"
          />

          <!-- selected range / active loop -->
          <div
            v-if="rangeBox" data-testid="range" class="absolute top-0 bottom-0 bg-yellow-400/20 border-x border-yellow-400 pointer-events-none z-[15]"
            :style="{ left: rangeBox.left + 'px', width: rangeBox.width + 'px' }"
          />

          <!-- playhead -->
          <div class="absolute top-0 bottom-0 w-px bg-accent pointer-events-none z-20" :style="{ left: store.playhead * pxPerMs + PAD + 'px' }">
            <div class="absolute -top-0 -left-[5px] w-[11px] h-3 bg-accent" style="clip-path: polygon(0 0, 100% 0, 50% 100%)" />
          </div>
        </div>
      </div>
     </div>
    </div>

    <!-- right-click menu -->
    <div
      v-if="menu" data-testid="clip-menu" role="menu" class="fixed z-50 min-w-44 py-1 rounded-md bg-panel-2 border border-line shadow-xl text-xs"
      :style="{ left: menu.x + 'px', top: menu.y + 'px' }"
    >
      <input
        v-if="menu.renaming" v-focus data-testid="clip-name" aria-label="Clip name" maxlength="80" :value="clipName(menu.clip)"
        class="mx-2 my-1 w-52 bg-panel border border-line rounded px-1.5 py-1 text-fg"
        @keydown.enter="commitName" @keydown.escape.stop="menu = null" @blur="commitName"
      />
      <template v-else>
        <button
          v-if="mergeCount >= 2" role="menuitem" data-testid="clip-merge" class="w-full text-left px-3 py-1.5 hover:bg-accent hover:text-black"
          title="Join the selected pieces of one file back into a single clip" @click="mergeSelected"
        >Merge {{ mergeCount }} clips</button>
        <button
          v-if="menu.kind === 'clip'"
          role="menuitem" class="w-full text-left px-3 py-1.5 hover:bg-accent hover:text-black disabled:opacity-40 disabled:hover:bg-transparent disabled:hover:text-fg"
          :disabled="!canDetach(menu.clip)" :title="canDetach(menu.clip) ? 'Move this clip\'s sound to an audio track and mute the video' : 'This clip has no audio'"
          @click="detachAudio"
        >Split audio from video</button>
        <button
          role="menuitem" data-testid="clip-rename" class="w-full text-left px-3 py-1.5 hover:bg-accent hover:text-black"
          title="Name this clip on the timeline (leave blank for the file name)" @click="menu.renaming = true"
        >Rename…</button>
        <button
          v-if="menu.kind === 'text'" role="menuitem" data-testid="clip-delete" class="w-full text-left px-3 py-1.5 hover:bg-accent hover:text-black"
          title="Remove this title (⌫ does the same)" @click="deleteFromMenu"
        >Delete</button>
      </template>
    </div>

    <!-- "+ Track" menu -->
    <div
      v-if="addMenu" data-testid="add-track-menu" role="menu" class="fixed z-50 min-w-36 py-1 rounded-md bg-panel-2 border border-line shadow-xl text-xs"
      :style="{ left: addMenu.x + 'px', top: addMenu.y + 'px' }"
    >
      <button
        v-for="t in TRACK_TYPES" :key="t.label" role="menuitem" class="w-full text-left px-3 py-1.5 hover:bg-accent hover:text-black"
        :title="t.hint" @click="addTrack(t.label)"
      >{{ t.label === 'Video' ? '▶' : t.label === 'Text' ? 'T' : '♪' }} {{ t.label }} track</button>
    </div>
  </div>
</template>
