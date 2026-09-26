<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { useProjectStore } from "../stores/project";
import ClipBlock from "./ClipBlock.vue";
import FreeBlock from "./FreeBlock.vue";
import MediaPool from "./MediaPool.vue";
import MuteToggle from "./MuteToggle.vue";
import { clipDuration, clipEnd, mediaKind } from "../types/project";
import type { AudioClip, Clip, OverlayClip, PoolItem } from "../types/project";
import { clamp, fmtMs } from "../utils/time";

const store = useProjectStore();
const scroller = ref<HTMLDivElement | null>(null);
const zoom = ref(1); // multiplier over "fit"
const viewW = ref(800);
const PAD = 24;
const MIN_CLIP = 100;
const SNAP_PX = 8;

/** Row heights (px). The label gutter on the left mirrors these exactly. */
const ROW = { ruler: 24, overlay: 44, video: 78, audio: 32, gap: 6 } as const;

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
  | { kind: "scrub" };
const drag = ref<Drag | null>(null);

const displayClips = computed(() => {
  const d = drag.value;
  if (!d || d.kind === "scrub" || d.kind.startsWith("free")) return store.clips;
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
function startScrub(e: PointerEvent) {
  drag.value = { kind: "scrub" };
  store.playing = false;
  store.seek(xToMs(e.clientX));
  capture(e);
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
  const dms = (e.clientX - d.x0) / pxPerMs.value;
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
  if (!d || d.kind === "scrub") return;
  if (d.kind === "move") {
    if (!d.moved) return;
    const centre = d.clip.timeline_start + d.dx + clipDuration(d.clip) / 2;
    let to = 0;
    for (const c of store.clips) if (c.id !== d.clip.id && c.timeline_start + clipDuration(c) / 2 < centre) to++;
    const from = store.clips.findIndex((c) => c.id === d.clip.id);
    if (to !== from) await store.moveClip(d.clip.id, to);
    return;
  }
  if (d.kind === "freeMove") {
    if (!d.moved) return;
    const at = Math.round(snapFree(d.free.clip.timeline_start + d.dx, clipDuration(d.free.clip)));
    if (d.free.kind === "overlay") await store.overlayMove(d.free.clip.id, at, d.layer ?? d.free.clip.layer);
    else await store.audioClipMove(d.free.clip.id, d.trackId ?? d.free.trackId, at);
    return;
  }
  if (!("free" in d)) {
    if (d.s !== d.s0 || d.e !== d.e0) await store.trim(d.clip.id, Math.round(d.s), Math.round(d.e));
    return;
  }
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
  if (!poolAccepts(row, item)) return;
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

const ro = new ResizeObserver(([e]) => { viewW.value = e.contentRect.width; });
onMounted(() => { if (scroller.value) ro.observe(scroller.value); });

// ---- audio track gutter ----
const editingLabel = ref<string | null>(null);
function commitLabel(trackId: string, e: Event, muted: boolean) {
  editingLabel.value = null;
  void store.audioTrackUpdate(trackId, (e.target as HTMLInputElement).value, muted);
}
const overlaysH = computed(() => store.overlayLayers * (ROW.overlay + ROW.gap));
const videoTop = computed(() => ROW.ruler + ROW.gap + overlaysH.value);
const audioTop = computed(() => videoTop.value + ROW.video + ROW.gap);
const tracksHeight = computed(() => audioTop.value + store.audioTracks.length * (ROW.audio + ROW.gap) + 24);
const isSelected = (kind: "clip" | "overlay" | "audio", id: string) => store.selected?.kind === kind && store.selected.id === id;
const rowClass = (ok: boolean | undefined) => (ok === undefined ? "" : ok ? "ring-1 ring-inset ring-accent/60 bg-accent/5" : "opacity-40");
</script>

<template>
  <div class="flex flex-col h-full bg-panel border-t border-line">
    <div class="flex items-center gap-3 px-3 h-8 shrink-0 border-b border-line text-xs text-muted">
      <span class="font-mono text-fg w-20">{{ fmtMs(store.playhead) }}</span>
      <span>/ {{ fmtMs(store.duration) }}</span>
      <span class="ml-2">{{ store.clips.length }} clip{{ store.clips.length === 1 ? '' : 's' }}</span>
      <span class="flex-1" />
      <span>Zoom</span>
      <input type="range" min="1" max="20" step="0.25" v-model.number="zoom" class="w-32" />
      <button class="hover:text-fg" @click="zoom = 1">Fit</button>
    </div>

    <div class="flex shrink-0 border-b border-line" :style="{ height: tracksHeight + 'px' }">
      <!-- label gutter, mirrors the row heights -->
      <div class="w-24 shrink-0 border-r border-line text-[10px] text-muted select-none">
        <div class="flex items-end justify-end px-1" :style="{ height: ROW.ruler + ROW.gap + 'px' }">
          <button class="hover:text-fg" title="Add an overlay layer for composite shots" @click="store.overlayLayerAdd()">+ Layer</button>
        </div>
        <div
          v-for="{ layer } in [...displayLayers].reverse()" :key="layer" class="px-2 flex items-center gap-1" :style="{ height: ROW.overlay + 'px', marginBottom: ROW.gap + 'px' }"
        >
          <span class="flex-1 truncate">V{{ layer + 2 }} · overlay</span>
          <button v-if="store.overlayLayers > 1" class="w-4 text-center hover:text-danger" :title="`Remove layer V${layer + 2}`" @click="store.overlayLayerRemove(layer)">✕</button>
        </div>
        <div class="px-2 flex items-center gap-1" :style="{ height: ROW.video + 'px', marginBottom: ROW.gap + 'px' }">
          <span class="flex-1 truncate">V1 · video</span>
          <MuteToggle :muted="!!store.project?.video_muted" label="video audio" :size="14" @toggle="store.setVideoMuted(!store.project?.video_muted)" />
        </div>
        <div v-for="{ track } in displayTracks" :key="track.id" class="px-1 flex items-center gap-1" :style="{ height: ROW.audio + 'px', marginBottom: ROW.gap + 'px' }">
          <input
            v-if="editingLabel === track.id" :value="track.label" list="track-presets" class="w-12 bg-panel-2 border border-line rounded px-1 text-fg" autofocus
            @change="commitLabel(track.id, $event, track.muted)" @blur="commitLabel(track.id, $event, track.muted)" @keydown.enter="($event.target as HTMLInputElement).blur()"
          />
          <button v-else class="flex-1 text-left truncate hover:text-fg" :title="'Rename ' + track.label" @click="editingLabel = track.id">♪ {{ track.label }}</button>
          <MuteToggle :muted="track.muted" label="track" :size="14" @toggle="store.audioTrackUpdate(track.id, track.label, !track.muted)" />
          <button class="w-4 text-center hover:text-danger" title="Remove track" @click="store.audioTrackRemove(track.id)">✕</button>
        </div>
        <button class="px-2 h-5 hover:text-fg" @click="store.audioTrackAdd('Audio')">+ Track</button>
        <datalist id="track-presets"><option value="Music" /><option value="SFX" /><option value="Narration" /><option value="Other" /></datalist>
      </div>

      <div
        ref="scroller" class="relative flex-1 overflow-x-auto overflow-y-hidden" style="cursor: default"
        @pointermove="onMove" @pointerup="onUp" @pointercancel="onUp"
      >
        <div class="relative h-full" :style="{ width: contentW + 'px' }">
          <!-- ruler -->
          <div class="absolute inset-x-0 top-0 border-b border-line cursor-text" :style="{ height: ROW.ruler + 'px' }" @pointerdown="startScrub">
            <div v-for="k in ticks" :key="k.t" class="absolute top-0 h-full border-l border-line/80 text-[10px] text-muted pl-1 pt-1" :style="{ left: k.x + PAD + 'px' }">{{ fmtMs(k.t, false) }}</div>
          </div>

          <!-- overlay layers: highest on top, V2 just above V1 -->
          <div
            v-for="{ layer, clips } in displayLayers" :key="layer"
            class="absolute inset-x-0 rounded-sm" :data-row="'overlay:' + layer" :class="rowClass(dropHint?.overlay)"
            :style="{ top: ROW.ruler + ROW.gap + (store.overlayLayers - 1 - layer) * (ROW.overlay + ROW.gap) + 'px', height: ROW.overlay + 'px', paddingLeft: PAD + 'px' }"
            @pointerdown="startScrub" @pooldrop="onPoolDrop('overlay:' + layer, $event as CustomEvent)"
          >
            <div class="relative h-full">
              <FreeBlock
                v-for="o in clips" :key="o.id" :clip="o" kind="overlay" :px-per-ms="pxPerMs" :selected="isSelected('overlay', o.id)"
                :thumbs="store.thumbs[o.source]" :dimmed="o.timeline_start >= store.duration"
                @select="store.select({ kind: 'overlay', id: o.id })"
                @trim-start="startFreeTrim({ kind: 'overlay', clip: o }, $event, 'freeTrimStart')" @trim-end="startFreeTrim({ kind: 'overlay', clip: o }, $event, 'freeTrimEnd')"
                @drag-start="startFreeMove({ kind: 'overlay', clip: o }, $event)"
              />
              <div v-if="!clips.length" class="text-[11px] text-muted/60 pt-3 pl-1 pointer-events-none">Drop a PNG or video here for logos, lower-thirds and B-roll</div>
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
                :selected="c.id === store.selectedClipId" :thumbs="store.thumbs[c.source]" :peaks="store.project?.video_muted ? undefined : store.waveforms[c.source]"
                @select="store.select(c.id)" @trim-start="startTrim(c, $event, 'trimStart')" @trim-end="startTrim(c, $event, 'trimEnd')" @drag-start="startMove(c, $event)"
              />
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
                @select="store.select({ kind: 'audio', id: c.id, trackId: track.id })"
                @trim-start="startFreeTrim({ kind: 'audio', clip: c, trackId: track.id }, $event, 'freeTrimStart')" @trim-end="startFreeTrim({ kind: 'audio', clip: c, trackId: track.id }, $event, 'freeTrimEnd')"
                @drag-start="startFreeMove({ kind: 'audio', clip: c, trackId: track.id }, $event)"
              />
            </div>
          </div>
          <div v-if="!displayTracks.length" class="absolute inset-x-0 text-[11px] text-muted/60 pl-7 pt-1 pointer-events-none" :style="{ top: audioTop + 'px' }">
            No audio tracks · click “+ Track”, then drop audio from the pool
          </div>

          <!-- playhead -->
          <div class="absolute top-0 bottom-0 w-px bg-accent pointer-events-none z-20" :style="{ left: store.playhead * pxPerMs + PAD + 'px' }">
            <div class="absolute -top-0 -left-[5px] w-[11px] h-3 bg-accent" style="clip-path: polygon(0 0, 100% 0, 50% 100%)" />
          </div>
        </div>
      </div>
    </div>

    <MediaPool class="flex-1 min-h-0" />
  </div>
</template>
