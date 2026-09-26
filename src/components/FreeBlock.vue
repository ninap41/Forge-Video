<script setup lang="ts">
// A free-positioned clip on the overlay (V2) or an audio track. Thin sibling of ClipBlock.
import { computed, onMounted, ref, watch } from "vue";
import type { AudioClip, OverlayClip } from "../types/project";
import { clipDuration } from "../types/project";
import type { Thumbs } from "../stores/project";
import { api } from "../api/tauri";
import { basename } from "../utils/time";
import { drawPeaks } from "../utils/waveform";

const props = defineProps<{
  clip: OverlayClip | AudioClip; kind: "overlay" | "audio"; pxPerMs: number; selected: boolean;
  thumbs?: Thumbs; peaks?: number[]; dimmed?: boolean;
}>();
const emit = defineEmits<{ select: []; trimStart: [e: PointerEvent]; trimEnd: [e: PointerEvent]; dragStart: [e: PointerEvent] }>();

const width = computed(() => clipDuration(props.clip) * props.pxPerMs);
const still = computed(() => props.clip.media.is_still);
const audio = computed(() => (props.kind === "audio" ? (props.clip as AudioClip) : null));
const stillUrl = computed(() => (still.value ? api.assetUrl(props.clip.source) : ""));

const visibleThumbs = computed(() => {
  const t = props.thumbs; if (still.value || !t || !t.urls.length) return [] as { url: string; left: number; w: number }[];
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
  const c = canvas.value, a = audio.value; if (!c || !a) return;
  drawPeaks(c, a.muted ? undefined : props.peaks, a.source_start, a.source_end, width.value, 28, a.volume, "rgba(59,130,246,0.9)");
}
onMounted(draw);
watch(() => [width.value, props.peaks, props.clip.source_start, props.clip.source_end, audio.value?.volume, audio.value?.muted], draw);
</script>

<template>
  <div
    class="absolute top-0 h-full rounded-md overflow-hidden border select-none group"
    :class="[selected ? 'border-accent ring-2 ring-accent/40 z-10' : 'border-line hover:border-muted', dimmed ? 'opacity-50' : '',
      kind === 'audio' ? 'bg-accent-2/15' : 'bg-panel-2']"
    :style="{ left: clip.timeline_start * pxPerMs + 'px', width: width + 'px' }"
    :data-clip-id="clip.id"
    @pointerdown.stop="emit('select'); emit('dragStart', $event)"
  >
    <template v-if="kind === 'overlay'">
      <div v-if="still" class="absolute inset-0 bg-black/40" :style="{ backgroundImage: `url(${stillUrl})`, backgroundSize: 'auto 100%', backgroundRepeat: 'repeat-x', backgroundPosition: 'left center' }" />
      <div v-else class="absolute inset-0 overflow-hidden bg-black/40">
        <img v-for="t in visibleThumbs" :key="t.left" :src="t.url" class="absolute top-0 h-full object-cover pointer-events-none" :style="{ left: t.left + 'px', width: t.w + 'px' }" draggable="false" />
      </div>
    </template>
    <canvas v-else ref="canvas" class="absolute inset-0 h-full" />
    <div class="absolute left-1.5 top-0.5 text-[10px] font-medium text-white drop-shadow px-1 rounded bg-black/40 truncate max-w-[calc(100%-12px)]">
      {{ kind === 'audio' ? '♪ ' : still ? '▣ ' : '' }}{{ basename(clip.source) }}
    </div>
    <div v-if="clip.fade_in" class="absolute left-0 top-0 h-full bg-gradient-to-r from-black/80 to-transparent pointer-events-none" :style="{ width: clip.fade_in * pxPerMs + 'px' }" />
    <div v-if="clip.fade_out" class="absolute right-0 top-0 h-full bg-gradient-to-l from-black/80 to-transparent pointer-events-none" :style="{ width: clip.fade_out * pxPerMs + 'px' }" />
    <div class="absolute left-0 top-0 h-full w-2 cursor-ew-resize bg-accent/0 group-hover:bg-accent/60" @pointerdown.stop="emit('select'); emit('trimStart', $event)" />
    <div class="absolute right-0 top-0 h-full w-2 cursor-ew-resize bg-accent/0 group-hover:bg-accent/60" @pointerdown.stop="emit('select'); emit('trimEnd', $event)" />
  </div>
</template>
