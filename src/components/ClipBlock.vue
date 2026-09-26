<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import type { Clip } from "../types/project";
import { clipDuration, transitionMs } from "../types/project";
import type { Thumbs } from "../stores/project";
import { basename } from "../utils/time";

const props = defineProps<{ clip: Clip; pxPerMs: number; selected: boolean; thumbs?: Thumbs; peaks?: number[]; index: number }>();
const emit = defineEmits<{ select: []; trimStart: [e: PointerEvent]; trimEnd: [e: PointerEvent]; dragStart: [e: PointerEvent] }>();

const width = computed(() => clipDuration(props.clip) * props.pxPerMs);
const trans = computed(() => transitionMs(props.clip.transition_out) * props.pxPerMs);

const visibleThumbs = computed(() => {
  const t = props.thumbs; if (!t || !t.urls.length) return [] as { url: string; left: number; w: number }[];
  const out = [];
  const first = Math.floor(props.clip.source_start / t.intervalMs);
  const last = Math.floor((props.clip.source_end - 1) / t.intervalMs);
  const w = t.intervalMs * props.pxPerMs;
  for (let i = first; i <= last && i < t.urls.length; i++) {
    out.push({ url: t.urls[i], left: (i * t.intervalMs - props.clip.source_start) * props.pxPerMs, w });
  }
  return out;
});

const canvas = ref<HTMLCanvasElement | null>(null);
function draw() {
  const c = canvas.value; if (!c) return;
  const w = Math.max(1, Math.floor(width.value)), h = 22;
  c.width = w; c.height = h;
  const ctx = c.getContext("2d")!; ctx.clearRect(0, 0, w, h);
  const p = props.peaks; if (!p || props.clip.muted) return;
  ctx.fillStyle = "rgba(245,165,36,0.85)";
  const s0 = props.clip.source_start / 10, s1 = props.clip.source_end / 10;
  const bucketsPerPx = (s1 - s0) / w;
  for (let x = 0; x < w; x++) {
    let m = 0;
    const a = Math.floor(s0 + x * bucketsPerPx), b = Math.max(a + 1, Math.floor(s0 + (x + 1) * bucketsPerPx));
    for (let i = a; i < b && i < p.length; i++) m = Math.max(m, p[i]);
    const bh = Math.max(1, (m / 255) * h * Math.min(1, props.clip.volume));
    ctx.fillRect(x, (h - bh) / 2, 1, bh);
  }
}
onMounted(draw);
watch(() => [width.value, props.peaks, props.clip.source_start, props.clip.source_end, props.clip.volume, props.clip.muted], draw);
</script>

<template>
  <div
    class="absolute top-0 h-full rounded-md overflow-hidden border select-none group"
    :class="selected ? 'border-accent ring-2 ring-accent/40 z-10' : 'border-line hover:border-muted'"
    :style="{ left: clip.timeline_start * pxPerMs + 'px', width: width + 'px', background: '#2a2a30' }"
    @pointerdown.stop="emit('select'); emit('dragStart', $event)"
  >
    <div class="absolute inset-x-0 top-0 h-[54px] overflow-hidden bg-black/40">
      <img v-for="t in visibleThumbs" :key="t.left" :src="t.url" class="absolute top-0 h-full object-cover pointer-events-none" :style="{ left: t.left + 'px', width: t.w + 'px' }" draggable="false" />
    </div>
    <canvas ref="canvas" class="absolute inset-x-0 bottom-0 h-[22px] bg-black/30" />
    <div class="absolute left-1.5 top-1 text-[11px] font-medium text-white drop-shadow px-1 rounded bg-black/40 truncate max-w-[calc(100%-12px)]">
      {{ index + 1 }} · {{ basename(clip.source) }}
    </div>
    <div v-if="clip.fade_in" class="absolute left-0 top-0 h-[54px] bg-gradient-to-r from-black/80 to-transparent pointer-events-none" :style="{ width: clip.fade_in * pxPerMs + 'px' }" />
    <div v-if="clip.fade_out" class="absolute right-0 top-0 h-[54px] bg-gradient-to-l from-black/80 to-transparent pointer-events-none" :style="{ width: clip.fade_out * pxPerMs + 'px' }" />
    <div v-if="trans > 0" class="absolute right-0 top-0 h-full bg-accent-2/40 border-l border-accent-2 pointer-events-none flex items-end justify-center text-[9px] text-white/80" :style="{ width: trans + 'px' }">
      {{ clip.transition_out.type === 'CrossDissolve' ? '⨯' : '■' }}
    </div>
    <div class="absolute left-0 top-0 h-full w-2 cursor-ew-resize bg-accent/0 group-hover:bg-accent/60" @pointerdown.stop="emit('select'); emit('trimStart', $event)" />
    <div class="absolute right-0 top-0 h-full w-2 cursor-ew-resize bg-accent/0 group-hover:bg-accent/60" @pointerdown.stop="emit('select'); emit('trimEnd', $event)" />
  </div>
</template>
