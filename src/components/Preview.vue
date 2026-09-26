<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from "vue";
import { useProjectStore } from "../stores/project";
import { api } from "../api/tauri";
import { ASPECT_PRESETS, clipDuration, transitionMs } from "../types/project";
import { clamp } from "../utils/time";

const store = useProjectStore();
const video = ref<HTMLVideoElement | null>(null);
const music = ref<HTMLAudioElement | null>(null);
const stage = ref<HTMLDivElement | null>(null);
const stageSize = ref({ w: 0, h: 0 });

const preset = computed(() => ASPECT_PRESETS.find((p) => p.id === store.project?.aspect) ?? ASPECT_PRESETS[0]);
const ratio = computed(() => preset.value.w / preset.value.h);

// Output frame letterboxed inside the stage.
const frameSize = computed(() => {
  const { w, h } = stageSize.value;
  if (!w || !h) return { w: 0, h: 0 };
  const fw = Math.min(w, h * ratio.value);
  return { w: fw, h: fw / ratio.value };
});

const cur = computed(() => store.current);
const src = computed(() => (cur.value ? api.assetUrl(cur.value.clip.source) : ""));

// Local crop while dragging; committed to Rust on pointer-up.
const localCrop = ref(store.project?.crop ?? { scale: 1, x: 0.5, y: 0.5 });
watch(() => store.project?.crop, (c) => { if (c && !dragging.value) localCrop.value = { ...c }; }, { deep: true });

function displaySize() {
  const m = cur.value?.clip.media;
  if (!m || !m.width) return { sw: 1920, sh: 1080 };
  return m.rotation % 180 ? { sw: m.height, sh: m.width } : { sw: m.width, sh: m.height };
}

/** Mirrors render::graph::crop_scale_filter so the preview matches the export. */
const videoStyle = computed(() => {
  const { w: W } = frameSize.value;
  const { sw, sh } = displaySize();
  const c = localCrop.value;
  const baseW = Math.min(sw, sh * ratio.value);
  const baseH = baseW / ratio.value;
  const scale = Math.max(1, c.scale);
  const cw = baseW / scale, ch = baseH / scale;
  const cx = clamp(c.x * sw, cw / 2, sw - cw / 2);
  const cy = clamp(c.y * sh, ch / 2, sh - ch / 2);
  const k = W / cw;
  return {
    width: `${sw * k}px`, height: `${sh * k}px`,
    left: `${-(cx - cw / 2) * k}px`, top: `${-(cy - ch / 2) * k}px`,
    opacity: String(fadeOpacity.value),
  };
});

const fadeOpacity = computed(() => {
  const c = cur.value; if (!c) return 1;
  const off = store.playhead - c.clip.timeline_start;
  const d = clipDuration(c.clip);
  let o = 1;
  if (c.clip.fade_in > 0 && off < c.clip.fade_in) o = Math.min(o, off / c.clip.fade_in);
  if (c.clip.fade_out > 0 && d - off < c.clip.fade_out) o = Math.min(o, (d - off) / c.clip.fade_out);
  const t = transitionMs(c.clip.transition_out);
  if (t > 0 && d - off < t) o = Math.min(o, (d - off) / t);
  const prev = store.clips[c.index - 1];
  const tp = prev ? transitionMs(prev.transition_out) : 0;
  if (tp > 0 && off < tp) o = Math.min(o, off / tp);
  return clamp(o, 0, 1);
});

// ---- crop drag / zoom ----
const dragging = ref(false);
let dragStart = { x: 0, y: 0, cx: 0, cy: 0 };
function onPointerDown(e: PointerEvent) {
  if (!cur.value) return;
  dragging.value = true;
  dragStart = { x: e.clientX, y: e.clientY, cx: localCrop.value.x, cy: localCrop.value.y };
  (e.target as HTMLElement).setPointerCapture(e.pointerId);
}
function onPointerMove(e: PointerEvent) {
  if (!dragging.value) return;
  const { sw, sh } = displaySize();
  const { w: W } = frameSize.value;
  const baseW = Math.min(sw, sh * ratio.value);
  const k = W / (baseW / Math.max(1, localCrop.value.scale));
  localCrop.value = {
    ...localCrop.value,
    x: clamp(dragStart.cx - (e.clientX - dragStart.x) / (sw * k), 0, 1),
    y: clamp(dragStart.cy - (e.clientY - dragStart.y) / (sh * k), 0, 1),
  };
}
function onPointerUp() {
  if (!dragging.value) return;
  dragging.value = false;
  void store.setCrop({ ...localCrop.value });
}
let wheelTimer: number | undefined;
function onWheel(e: WheelEvent) {
  if (!cur.value) return;
  e.preventDefault();
  localCrop.value = { ...localCrop.value, scale: clamp(localCrop.value.scale * (e.deltaY < 0 ? 1.05 : 0.95), 1, 4) };
  window.clearTimeout(wheelTimer);
  wheelTimer = window.setTimeout(() => void store.setCrop({ ...localCrop.value }), 200);
}

// ---- playback ----
let raf = 0;
let lastClipId: string | null = null;

function syncSeek() {
  const v = video.value, c = cur.value;
  if (!v || !c) return;
  const t = c.sourceMs / 1000;
  if (Math.abs(v.currentTime - t) > 0.03) v.currentTime = t;
  v.volume = c.clip.muted ? 0 : clamp(c.clip.volume, 0, 1);
  syncMusic();
}
function syncMusic() {
  const a = music.value, m = store.project?.music;
  if (!a) return;
  if (!m || m.muted) { a.pause(); return; }
  const off = store.playhead - m.timeline_start;
  const inRange = off >= 0 && off < m.trim_end - m.trim_start;
  a.volume = clamp(m.volume, 0, 1);
  if (!inRange) { a.pause(); return; }
  const t = (m.trim_start + off) / 1000;
  if (Math.abs(a.currentTime - t) > 0.08) a.currentTime = t;
  if (store.playing && a.paused) void a.play().catch(() => {});
}

function tick() {
  const v = video.value, c = cur.value;
  if (!v || !c || !store.playing) return;
  const srcMs = v.currentTime * 1000;
  if (srcMs >= c.clip.source_end - 20 || v.ended) {
    const next = store.clips[c.index + 1];
    if (next) { store.playhead = next.timeline_start; }
    else { store.playing = false; store.playhead = Math.max(store.duration - 1, 0); return; }
  } else {
    store.playhead = c.clip.timeline_start + Math.max(0, srcMs - c.clip.source_start);
  }
  syncMusic();
  raf = requestAnimationFrame(tick);
}

watch(() => store.playing, async (p) => {
  const v = video.value; if (!v) return;
  if (p) {
    if (store.playhead >= store.duration - 1) store.playhead = 0;
    syncSeek();
    await v.play().catch(() => {});
    raf = requestAnimationFrame(tick);
  } else {
    v.pause(); music.value?.pause(); cancelAnimationFrame(raf);
  }
});

// External seeks (scrub) while paused, and clip changes while playing.
watch(() => [store.playhead, cur.value?.clip.id] as const, async ([, id]) => {
  const v = video.value; if (!v) return;
  const clipChanged = id !== lastClipId;
  lastClipId = id ?? null;
  if (clipChanged && store.playing) {
    await nextSrcReady();
    syncSeek();
    await v.play().catch(() => {});
  } else if (!store.playing) {
    if (clipChanged) await nextSrcReady();
    syncSeek();
  }
});
function nextSrcReady() {
  return new Promise<void>((res) => {
    const v = video.value; if (!v) return res();
    if (v.readyState >= 1) return res();
    v.addEventListener("loadedmetadata", () => res(), { once: true });
  });
}
watch(() => store.project?.music?.source, () => { if (music.value) music.value.load(); });
watch(() => [store.project?.music?.volume, store.project?.music?.muted], syncMusic);

const ro = new ResizeObserver(([e]) => { stageSize.value = { w: e.contentRect.width, h: e.contentRect.height }; });
watch(stage, (s, old) => { if (old) ro.unobserve(old); if (s) ro.observe(s); });
onBeforeUnmount(() => { ro.disconnect(); cancelAnimationFrame(raf); });
</script>

<template>
  <div ref="stage" class="relative flex-1 min-h-0 flex items-center justify-center bg-black/60 overflow-hidden">
    <div v-if="!cur" class="text-muted text-sm text-center px-8">
      Drop a video here, or click <b>Import</b>.<br /><span class="text-xs">Trim · Split · Fade · Dissolve · Music · Aspect · Export</span>
    </div>
    <div
      v-else
      class="relative overflow-hidden bg-black shadow-2xl ring-1 ring-line cursor-grab active:cursor-grabbing"
      :style="{ width: frameSize.w + 'px', height: frameSize.h + 'px' }"
      @pointerdown="onPointerDown" @pointermove="onPointerMove" @pointerup="onPointerUp" @pointercancel="onPointerUp" @wheel="onWheel"
    >
      <video ref="video" :src="src" class="absolute max-w-none pointer-events-none" :style="videoStyle" playsinline preload="auto" @loadedmetadata="syncSeek" />
      <div class="absolute bottom-1 right-2 text-[10px] text-white/60 bg-black/40 px-1.5 rounded">{{ preset.label }} {{ preset.sub }} · drag to reposition · scroll to zoom</div>
    </div>
    <audio ref="music" :src="store.project?.music ? api.assetUrl(store.project.music.source) : undefined" preload="auto" />
  </div>
</template>
