<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { useProjectStore } from "../stores/project";
import ClipBlock from "./ClipBlock.vue";
import { clipDuration } from "../types/project";
import type { Clip } from "../types/project";
import { clamp, fmtMs } from "../utils/time";

const store = useProjectStore();
const scroller = ref<HTMLDivElement | null>(null);
const zoom = ref(1); // multiplier over "fit"
const viewW = ref(800);
const PAD = 24;
const MIN_CLIP = 100;

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
type Drag =
  | { kind: "trimStart" | "trimEnd"; clip: Clip; x0: number; s0: number; e0: number; s: number; e: number }
  | { kind: "move"; clip: Clip; x0: number; dx: number; moved: boolean }
  | { kind: "scrub" };
const drag = ref<Drag | null>(null);

const displayClips = computed(() => {
  const d = drag.value;
  if (!d || d.kind === "scrub") return store.clips;
  if (d.kind === "move") {
    return store.clips.map((c) => (c.id === d.clip.id ? { ...c, timeline_start: c.timeline_start + d.dx } : c));
  }
  // Trim: show the edited clip and ripple the rest locally.
  let cursor = 0;
  return store.clips.map((c) => {
    const cc = c.id === d.clip.id ? { ...c, source_start: d.s, source_end: d.e } : { ...c };
    cc.timeline_start = cursor;
    cursor += clipDuration(cc);
    cursor -= cc.transition_out.type === "None" ? 0 : cc.transition_out.ms;
    return cc;
  });
});

function xToMs(clientX: number) {
  const el = scroller.value!; const r = el.getBoundingClientRect();
  return (clientX - r.left + el.scrollLeft - PAD) / pxPerMs.value;
}

function startTrim(c: Clip, e: PointerEvent, kind: "trimStart" | "trimEnd") {
  drag.value = { kind, clip: c, x0: e.clientX, s0: c.source_start, e0: c.source_end, s: c.source_start, e: c.source_end };
  capture(e);
}
function startMove(c: Clip, e: PointerEvent) {
  drag.value = { kind: "move", clip: c, x0: e.clientX, dx: 0, moved: false };
  capture(e);
}
function startScrub(e: PointerEvent) {
  drag.value = { kind: "scrub" };
  store.playing = false;
  store.seek(xToMs(e.clientX));
  capture(e);
}
function capture(e: PointerEvent) { (scroller.value as HTMLElement).setPointerCapture(e.pointerId); }

function onMove(e: PointerEvent) {
  const d = drag.value; if (!d) return;
  if (d.kind === "scrub") { store.seek(xToMs(e.clientX)); return; }
  const dms = (e.clientX - d.x0) / pxPerMs.value;
  if (d.kind === "move") { d.dx = dms; if (Math.abs(e.clientX - d.x0) > 4) d.moved = true; return; }
  const frame = 1000 / Math.max(1, d.clip.media.fps.num / Math.max(1, d.clip.media.fps.den));
  const snap = (v: number) => Math.round(v / frame) * frame;
  if (d.kind === "trimStart") d.s = clamp(snap(d.s0 + dms), 0, d.e0 - MIN_CLIP);
  else d.e = clamp(snap(d.e0 + dms), d.s0 + MIN_CLIP, d.clip.media.duration_ms);
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
  if (d.s !== d.s0 || d.e !== d.e0) await store.trim(d.clip.id, Math.round(d.s), Math.round(d.e));
}

// Keep playhead visible during playback.
watch(() => store.playhead, (t) => {
  const el = scroller.value; if (!el || !store.playing) return;
  const x = t * pxPerMs.value + PAD;
  if (x < el.scrollLeft + 40 || x > el.scrollLeft + el.clientWidth - 40) el.scrollLeft = x - el.clientWidth / 3;
});

const ro = new ResizeObserver(([e]) => { viewW.value = e.contentRect.width; });
onMounted(() => { if (scroller.value) ro.observe(scroller.value); });

const music = computed(() => store.project?.music ?? null);
const musicPeaks = computed(() => (music.value ? store.waveforms[music.value.source] : undefined));
const musicCanvas = ref<HTMLCanvasElement | null>(null);
function drawMusic() {
  const c = musicCanvas.value, m = music.value; if (!c || !m) return;
  const w = Math.max(1, Math.floor((m.trim_end - m.trim_start) * pxPerMs.value)), h = 28;
  c.width = w; c.height = h;
  const ctx = c.getContext("2d")!; ctx.clearRect(0, 0, w, h);
  const p = musicPeaks.value; if (!p || m.muted) return;
  ctx.fillStyle = "rgba(59,130,246,0.9)";
  const s0 = m.trim_start / 10, s1 = m.trim_end / 10, bpp = (s1 - s0) / w;
  for (let x = 0; x < w; x++) {
    let mx = 0;
    for (let i = Math.floor(s0 + x * bpp); i < Math.max(Math.floor(s0 + x * bpp) + 1, Math.floor(s0 + (x + 1) * bpp)) && i < p.length; i++) mx = Math.max(mx, p[i]);
    const bh = Math.max(1, (mx / 255) * h * Math.min(1, m.volume));
    ctx.fillRect(x, (h - bh) / 2, 1, bh);
  }
}
watch([music, musicPeaks, pxPerMs], () => requestAnimationFrame(drawMusic), { deep: true });
onMounted(drawMusic);
</script>

<template>
  <div class="flex flex-col h-full bg-panel border-t border-line">
    <div class="flex items-center gap-3 px-3 h-8 border-b border-line text-xs text-muted">
      <span class="font-mono text-fg w-20">{{ fmtMs(store.playhead) }}</span>
      <span>/ {{ fmtMs(store.duration) }}</span>
      <span class="ml-2">{{ store.clips.length }} clip{{ store.clips.length === 1 ? '' : 's' }}</span>
      <span class="flex-1" />
      <span>Zoom</span>
      <input type="range" min="1" max="20" step="0.25" v-model.number="zoom" class="w-32" />
      <button class="hover:text-fg" @click="zoom = 1">Fit</button>
    </div>

    <div
      ref="scroller" class="relative flex-1 overflow-x-auto overflow-y-hidden" style="cursor: default"
      @pointermove="onMove" @pointerup="onUp" @pointercancel="onUp"
    >
      <div class="relative h-full" :style="{ width: contentW + 'px' }">
        <!-- ruler -->
        <div class="absolute inset-x-0 top-0 h-6 border-b border-line cursor-text" @pointerdown="startScrub">
          <div v-for="k in ticks" :key="k.t" class="absolute top-0 h-full border-l border-line/80 text-[10px] text-muted pl-1 pt-1" :style="{ left: k.x + PAD + 'px' }">{{ fmtMs(k.t, false) }}</div>
        </div>

        <!-- video track -->
        <div class="absolute inset-x-0 top-8 h-[78px]" :style="{ paddingLeft: PAD + 'px' }" @pointerdown="startScrub">
          <div class="relative h-full">
            <ClipBlock
              v-for="(c, i) in displayClips" :key="c.id" :clip="c" :index="i" :px-per-ms="pxPerMs"
              :selected="c.id === store.selectedClipId" :thumbs="store.thumbs[c.source]" :peaks="store.waveforms[c.source]"
              @select="store.select(c.id)" @trim-start="startTrim(c, $event, 'trimStart')" @trim-end="startTrim(c, $event, 'trimEnd')" @drag-start="startMove(c, $event)"
            />
          </div>
        </div>

        <!-- music track -->
        <div class="absolute inset-x-0 top-[120px] h-8" :style="{ paddingLeft: PAD + 'px' }" @pointerdown="startScrub">
          <div v-if="music" class="absolute h-full rounded-md border border-accent-2/60 bg-accent-2/15 overflow-hidden" :style="{ left: PAD + music.timeline_start * pxPerMs + 'px', width: (music.trim_end - music.trim_start) * pxPerMs + 'px' }">
            <canvas ref="musicCanvas" class="absolute inset-0 h-full" />
            <span class="absolute left-1.5 top-0.5 text-[10px] text-white/80">♪ music</span>
          </div>
          <div v-else class="text-[11px] text-muted/60 pt-2 pl-1">No music · add one in the panel on the right</div>
        </div>

        <!-- playhead -->
        <div class="absolute top-0 bottom-0 w-px bg-accent pointer-events-none z-20" :style="{ left: store.playhead * pxPerMs + PAD + 'px' }">
          <div class="absolute -top-0 -left-[5px] w-[11px] h-3 bg-accent" style="clip-path: polygon(0 0, 100% 0, 50% 100%)" />
        </div>
      </div>
    </div>
  </div>
</template>
