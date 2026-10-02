<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from "vue";
import { useProjectStore } from "../stores/project";
import { api } from "../api/tauri";
import { ASPECT_PRESETS, TEXT_SIZE_MAX, TEXT_SIZE_MIN, clipDuration, transitionMs, type OverlayClip, type Placement, type TextClip, CROP_SCALE_MIN, CROP_SCALE_MAX } from "../types/project";
import { rasterText } from "../utils/textRaster";
import { basename, clamp } from "../utils/time";

const store = useProjectStore();
const video = ref<HTMLVideoElement | null>(null);
const overlayEls = ref<Record<string, HTMLVideoElement>>({});
function setOverlayEl(id: string, el: unknown) {
  if (el) overlayEls.value[id] = el as HTMLVideoElement; else delete overlayEls.value[id];
}
const audioEls = ref<Record<string, HTMLAudioElement>>({});
function setAudioEl(id: string, el: unknown) {
  if (el) audioEls.value[id] = el as HTMLAudioElement; else delete audioEls.value[id];
}
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
const ovs = computed(() => store.currentOverlays);
/** The selected overlay, if it is on screen right now: the one preview drags act on. */
const ov = computed(() => ovs.value.find((o) => store.selected?.kind === "overlay" && store.selected.id === o.clip.id) ?? null);
const ovSelected = computed(() => !!ov.value);
const txs = computed(() => store.currentTexts);
/** The selected title, if it is on screen right now: preview drags move it, the wheel resizes it. */
const tx = computed(() => txs.value.find((t) => store.selected?.kind === "text" && store.selected.id === t.clip.id) ?? null);
const txSelected = computed(() => !!tx.value);
const curStill = computed(() => !!cur.value?.clip.media.is_still);

// Local crop while dragging; committed to Rust on pointer-up.
const localCrop = ref(store.project?.crop ?? { scale: 1, x: 0.5, y: 0.5 });
watch(() => store.project?.crop, (c) => { if (c && !dragging.value) localCrop.value = { ...c }; }, { deep: true });

function displaySize() {
  const m = cur.value?.clip.media;
  if (!m || !m.width) return { sw: 1920, sh: 1080 };
  return m.rotation % 180 ? { sw: m.height, sh: m.width } : { sw: m.width, sh: m.height };
}

/** Mirrors render::graph::crop_scale_filter so the preview matches the export: the source point
 *  (x, y) lands at the frame centre; zoomed in it is clamped so the frame stays inside the source,
 *  zoomed out (scale < 1) the small picture may sit partly outside the frame. */
const videoStyle = computed(() => {
  const { w: W, h: H } = frameSize.value;
  const { sw, sh } = displaySize();
  const c = localCrop.value;
  const baseW = Math.min(sw, sh * ratio.value);
  const baseH = baseW / ratio.value;
  const scale = Math.max(CROP_SCALE_MIN, c.scale);
  const k = (W / baseW) * scale;
  const cw = baseW / scale, ch = baseH / scale;
  const cx = scale >= 1 ? clamp(c.x * sw, cw / 2, sw - cw / 2) : c.x * sw;
  const cy = scale >= 1 ? clamp(c.y * sh, ch / 2, sh - ch / 2) : c.y * sh;
  return {
    width: `${sw * k}px`, height: `${sh * k}px`,
    left: `${W / 2 - cx * k}px`, top: `${H / 2 - cy * k}px`,
    opacity: String(fadeOpacity.value),
  };
});

/** Opacity from a clip's own fades at an offset into it. */
function fadeAt(clip: { fade_in: number; fade_out: number; source_start: number; source_end: number }, off: number) {
  const d = clipDuration(clip);
  let o = 1;
  if (clip.fade_in > 0 && off < clip.fade_in) o = Math.min(o, off / clip.fade_in);
  if (clip.fade_out > 0 && d - off < clip.fade_out) o = Math.min(o, (d - off) / clip.fade_out);
  return o;
}

/** Mirrors render::graph::overlay_chain: width = frame width × scale, centred on (x, y). */
function overlayStyle(o: { clip: OverlayClip }) {
  const { w: W, h: H } = frameSize.value;
  if (!W) return {};
  const pl = (ov.value?.clip.id === o.clip.id && localPlacement.value) || o.clip.placement;
  const { sw, sh } = o.clip.media.rotation % 180 ? { sw: o.clip.media.height, sh: o.clip.media.width } : { sw: o.clip.media.width, sh: o.clip.media.height };
  const w = W * pl.scale, h = sw > 0 ? w * (sh / sw) : w * 9 / 16;
  return {
    width: `${w}px`, height: `${h}px`, left: `${W * pl.x - w / 2}px`, top: `${H * pl.y - h / 2}px`,
    opacity: String(clamp(fadeAt(o.clip, store.playhead - o.clip.timeline_start) * (o.clip.opacity ?? 1), 0, 1)),
  };
}

/** Same canvas raster as the export, at preview size, centred on (x, y). */
function textStyle(t: { clip: TextClip; opacity: number }) {
  const { w: W, h: H } = frameSize.value;
  if (!W) return {};
  const local = tx.value?.clip.id === t.clip.id ? localText.value : null;
  const style = local ? { ...t.clip.style, size: local.size } : t.clip.style;
  const pos = local ?? t.clip;
  const r = rasterText(style, W, H);
  return { width: `${r.w}px`, height: `${r.h}px`, left: `${W * pos.x - r.w / 2}px`, top: `${H * pos.y - r.h / 2}px`, opacity: String(t.opacity) };
}
function textSrc(t: { clip: TextClip }) {
  const { w: W, h: H } = frameSize.value;
  if (!W) return "";
  const local = tx.value?.clip.id === t.clip.id ? localText.value : null;
  return rasterText(local ? { ...t.clip.style, size: local.size } : t.clip.style, W, H).url;
}

const fadeOpacity = computed(() => {
  const c = cur.value; if (!c) return 1;
  const off = store.playhead - c.clip.timeline_start;
  const d = clipDuration(c.clip);
  let o = fadeAt(c.clip, off);
  const t = transitionMs(c.clip.transition_out);
  if (t > 0 && d - off < t) o = Math.min(o, (d - off) / t);
  const prev = store.clips[c.index - 1];
  const tp = prev ? transitionMs(prev.transition_out) : 0;
  if (tp > 0 && off < tp) o = Math.min(o, off / tp);
  return clamp(o, 0, 1);
});

// ---- crop drag / zoom ----
const dragging = ref(false);
// Local placement while dragging the selected overlay; committed on pointer-up.
const localPlacement = ref<Placement | null>(null);
watch(() => ov.value?.clip.placement, (p) => { if (!dragging.value) localPlacement.value = p ? { ...p } : null; }, { deep: true, immediate: true });

// Local position / size while dragging or scrolling the selected title; committed on release.
const localText = ref<{ x: number; y: number; size: number } | null>(null);
watch(() => tx.value?.clip, (t) => { if (!dragging.value) localText.value = t ? { x: t.x, y: t.y, size: t.style.size } : null; }, { deep: true, immediate: true });

let dragStart = { x: 0, y: 0, cx: 0, cy: 0, overlay: false, text: false };
function onPointerDown(e: PointerEvent) {
  if (!cur.value) return;
  dragging.value = true;
  const text = txSelected.value && !!localText.value;
  const overlay = !text && ovSelected.value && !!localPlacement.value;
  dragStart = text
    ? { x: e.clientX, y: e.clientY, cx: localText.value!.x, cy: localText.value!.y, overlay, text }
    : overlay
    ? { x: e.clientX, y: e.clientY, cx: localPlacement.value!.x, cy: localPlacement.value!.y, overlay, text }
    : { x: e.clientX, y: e.clientY, cx: localCrop.value.x, cy: localCrop.value.y, overlay, text };
  (e.target as HTMLElement).setPointerCapture(e.pointerId);
}
function onPointerMove(e: PointerEvent) {
  if (!dragging.value) return;
  if (dragStart.text) {
    const { w: W, h: H } = frameSize.value; if (!W || !localText.value) return;
    localText.value = { ...localText.value, x: clamp(dragStart.cx + (e.clientX - dragStart.x) / W, 0, 1), y: clamp(dragStart.cy + (e.clientY - dragStart.y) / H, 0, 1) };
    return;
  }
  if (dragStart.overlay) {
    const { w: W, h: H } = frameSize.value; if (!W || !localPlacement.value) return;
    localPlacement.value = { ...localPlacement.value, x: clamp(dragStart.cx + (e.clientX - dragStart.x) / W, 0, 1), y: clamp(dragStart.cy + (e.clientY - dragStart.y) / H, 0, 1) };
    return;
  }
  const { sw, sh } = displaySize();
  const { w: W } = frameSize.value;
  const baseW = Math.min(sw, sh * ratio.value);
  const k = (W / baseW) * Math.max(CROP_SCALE_MIN, localCrop.value.scale);
  localCrop.value = {
    ...localCrop.value,
    x: clamp(dragStart.cx - (e.clientX - dragStart.x) / (sw * k), 0, 1),
    y: clamp(dragStart.cy - (e.clientY - dragStart.y) / (sh * k), 0, 1),
  };
}
function onPointerUp() {
  if (!dragging.value) return;
  dragging.value = false;
  if (dragStart.text && tx.value && localText.value) void store.textSetPosition(tx.value.clip.id, localText.value.x, localText.value.y);
  else if (dragStart.overlay && ov.value && localPlacement.value) void store.overlaySetPlacement(ov.value.clip.id, { ...localPlacement.value });
  else void store.setCrop({ ...localCrop.value });
}
let wheelTimer: number | undefined;
function onWheel(e: WheelEvent) {
  if (!cur.value) return;
  e.preventDefault();
  if (txSelected.value && localText.value) {
    localText.value = { ...localText.value, size: clamp(localText.value.size * (e.deltaY < 0 ? 1.05 : 0.95), TEXT_SIZE_MIN, TEXT_SIZE_MAX) };
    window.clearTimeout(wheelTimer);
    wheelTimer = window.setTimeout(() => { if (tx.value && localText.value) void store.textSetStyle(tx.value.clip.id, { ...tx.value.clip.style, size: localText.value.size }); }, 200);
    return;
  }
  if (ovSelected.value && localPlacement.value) {
    localPlacement.value = { ...localPlacement.value, scale: clamp(localPlacement.value.scale * (e.deltaY < 0 ? 1.05 : 0.95), 0.05, 1) };
    window.clearTimeout(wheelTimer);
    wheelTimer = window.setTimeout(() => { if (ov.value && localPlacement.value) void store.overlaySetPlacement(ov.value.clip.id, { ...localPlacement.value }); }, 200);
    return;
  }
  localCrop.value = { ...localCrop.value, scale: clamp(localCrop.value.scale * (e.deltaY < 0 ? 1.05 : 0.95), CROP_SCALE_MIN, CROP_SCALE_MAX) };
  window.clearTimeout(wheelTimer);
  wheelTimer = window.setTimeout(() => void store.setCrop({ ...localCrop.value }), 200);
}

// ---- playback ----
let raf = 0;
let lastClipId: string | null = null;

function syncSeek() {
  const v = video.value, c = cur.value;
  if (!c) return;
  if (v && !curStill.value) {
    const t = c.sourceMs / 1000;
    if (Math.abs(v.currentTime - t) > 0.03) v.currentTime = t;
    v.volume = c.clip.muted || store.project?.video_muted ? 0 : clamp(c.clip.volume * (store.project?.video_volume ?? 1), 0, 1);
  }
  syncOverlay();
  syncAudio();
}
function syncOverlay() {
  for (const o of ovs.value) {
    const e = overlayEls.value[o.clip.id];
    if (!e || o.clip.media.is_still) continue;
    const t = o.sourceMs / 1000;
    if (Math.abs(e.currentTime - t) > 0.08) e.currentTime = t;
    e.muted = o.gain === 0;
    e.volume = o.gain;
    if (store.playing && e.paused) void e.play().catch(() => {});
    if (!store.playing && !e.paused) e.pause();
  }
}
/** One <audio> per audio clip under the playhead; elements come and go with the playhead. */
function syncAudio() {
  for (const { clip, track, sourceMs, silent } of store.activeAudioClips) {
    const a = audioEls.value[clip.id]; if (!a) continue;
    a.volume = silent ? 0 : clamp(clip.volume * track.volume, 0, 1);
    const t = sourceMs / 1000;
    if (Math.abs(a.currentTime - t) > 0.08) a.currentTime = t;
    if (store.playing && a.paused) void a.play().catch(() => {});
    if (!store.playing && !a.paused) a.pause();
  }
}

let stillClock = { at: 0, playhead: 0 };
/** Looping: once the playhead reaches the range end, jump back to its start. */
function wrapLoop(t: number): boolean {
  const r = store.loopRange;
  if (!r || t < r.end) return false;
  stillClock = { at: 0, playhead: 0 };
  store.playhead = r.start;
  syncSeek();
  raf = requestAnimationFrame(tick);
  return true;
}
function tick() {
  const v = video.value, c = cur.value;
  if (!c || !store.playing) return;
  // Stills have no media clock: advance the playhead by wall time instead.
  if (curStill.value || !v) {
    const now = performance.now();
    if (stillClock.at === 0) stillClock = { at: now, playhead: store.playhead };
    const t = stillClock.playhead + (now - stillClock.at);
    if (wrapLoop(t)) return;
    const end = c.clip.timeline_start + clipDuration(c.clip);
    if (t >= end) {
      stillClock = { at: 0, playhead: 0 };
      const next = store.clips[c.index + 1];
      if (next) { store.playhead = next.timeline_start; }
      else { store.playing = false; store.playhead = Math.max(store.duration - 1, 0); return; }
    } else {
      store.playhead = t;
    }
    syncOverlay();
    syncAudio();
    raf = requestAnimationFrame(tick);
    return;
  }
  stillClock = { at: 0, playhead: 0 };
  const srcMs = v.currentTime * 1000;
  if (wrapLoop(c.clip.timeline_start + Math.max(0, srcMs - c.clip.source_start))) return;
  if (srcMs >= c.clip.source_end - 20 || v.ended) {
    const next = store.clips[c.index + 1];
    if (next) { store.playhead = next.timeline_start; }
    else if (store.loopRange) { store.playhead = store.loopRange.start; }
    else { store.playing = false; store.playhead = Math.max(store.duration - 1, 0); return; }
  } else {
    store.playhead = c.clip.timeline_start + Math.max(0, srcMs - c.clip.source_start);
  }
  syncOverlay();
  syncAudio();
  raf = requestAnimationFrame(tick);
}

watch(() => store.playing, async (p) => {
  const v = video.value;
  if (p) {
    const r = store.loopRange;
    if (r && (store.playhead < r.start || store.playhead >= r.end)) store.playhead = r.start;
    else if (store.playhead >= store.duration - 1) store.playhead = 0;
    stillClock = { at: 0, playhead: 0 };
    syncSeek();
    if (v && !curStill.value) await v.play().catch(() => {});
    raf = requestAnimationFrame(tick);
  } else {
    v?.pause(); Object.values(overlayEls.value).forEach((e) => e.pause()); Object.values(audioEls.value).forEach((a) => a.pause()); cancelAnimationFrame(raf);
  }
});

// External seeks (scrub) while paused, and clip changes while playing.
watch(() => [store.playhead, cur.value?.clip.id] as const, async ([, id]) => {
  const clipChanged = id !== lastClipId;
  lastClipId = id ?? null;
  if (clipChanged) stillClock = { at: 0, playhead: 0 };
  const v = video.value;
  if (curStill.value) { if (clipChanged) { v?.pause(); syncSeek(); } return; }
  if (!v) return;
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
    if (v.readyState >= 1 || v.error) return res();
    const done = () => { v.removeEventListener("loadedmetadata", done); v.removeEventListener("error", done); res(); };
    v.addEventListener("loadedmetadata", done, { once: true });
    v.addEventListener("error", done, { once: true });
  });
}
// WKWebView cannot play everything ffmpeg can (mkv, avi, mts, some webm/ogg). Say so once per file
// instead of freezing; export is unaffected.
const unplayable = new Set<string>();
function onMediaError(source: string) {
  if (unplayable.has(source)) return;
  unplayable.add(source);
  store.notify(`Preview can't play ${basename(source)} here — export still works`, 5000);
}
// New audio elements (playhead entered a clip) and volume/mute edits need a nudge.
watch(() => store.activeAudioClips.map((a) => `${a.clip.id}:${a.clip.volume}:${a.track.volume}:${a.silent}`).join(","), () => { void Promise.resolve().then(syncAudio); });
watch(() => ovs.value.map((o) => `${o.clip.id}:${o.gain}`).join(","), () => { void Promise.resolve().then(syncOverlay); });
watch(() => [store.project?.video_muted, store.project?.video_volume], syncSeek);

const ro = new ResizeObserver(([e]) => { stageSize.value = { w: e.contentRect.width, h: e.contentRect.height }; });
watch(stage, (s, old) => { if (old) ro.unobserve(old); if (s) ro.observe(s); });
onBeforeUnmount(() => { ro.disconnect(); cancelAnimationFrame(raf); });
</script>

<template>
  <div ref="stage" class="relative flex-1 min-h-0 flex items-center justify-center bg-black/60 overflow-hidden">
    <div v-if="!cur" class="text-muted text-sm text-center px-8">
      Drop a video or image here, or click <b>Import</b>.<br /><span class="text-xs">Trim · Split · Fade · Dissolve · Overlays · Audio tracks · Aspect · Export</span>
    </div>
    <div
      v-else
      class="relative overflow-hidden bg-black shadow-2xl ring-1 ring-line cursor-grab active:cursor-grabbing"
      :style="{ width: frameSize.w + 'px', height: frameSize.h + 'px' }"
      @pointerdown="onPointerDown" @pointermove="onPointerMove" @pointerup="onPointerUp" @pointercancel="onPointerUp" @wheel="onWheel"
    >
      <img v-if="curStill" :src="src" class="absolute max-w-none pointer-events-none" :style="videoStyle" draggable="false" data-testid="still-layer" />
      <video v-else ref="video" :src="src" class="absolute max-w-none pointer-events-none" :style="videoStyle" playsinline preload="auto" @loadedmetadata="syncSeek" @error="store.current && onMediaError(store.current.clip.source)" />
      <template v-for="o in ovs" :key="o.clip.id">
        <img
          v-if="o.clip.media.is_still" :src="api.assetUrl(o.clip.source)" class="absolute max-w-none pointer-events-none object-fill"
          :class="ov?.clip.id === o.clip.id ? 'outline outline-1 outline-accent' : ''" :style="overlayStyle(o)" draggable="false" data-testid="overlay-layer"
        />
        <video
          v-else :ref="(el) => setOverlayEl(o.clip.id, el)" :src="api.assetUrl(o.clip.source)" class="absolute max-w-none pointer-events-none object-fill"
          :class="ov?.clip.id === o.clip.id ? 'outline outline-1 outline-accent' : ''" :style="overlayStyle(o)" playsinline preload="auto" :muted="o.gain === 0" data-testid="overlay-layer" @loadedmetadata="syncOverlay" @error="onMediaError(o.clip.source)"
        />
      </template>
      <img
        v-for="t in txs" :key="t.clip.id" :src="textSrc(t)" class="absolute max-w-none pointer-events-none"
        :class="tx?.clip.id === t.clip.id ? 'outline outline-1 outline-accent' : ''" :style="textStyle(t)" draggable="false" data-testid="text-layer"
      />
      <div
        v-if="store.currentCue" data-testid="caption" class="absolute inset-x-0 bottom-[12%] px-[6%] text-center font-semibold leading-snug pointer-events-none"
        :style="{ fontSize: Math.max(10, frameSize.h * 0.045) + 'px' }"
      ><span class="bg-black/65 text-white rounded px-[0.4em] py-[0.1em] [box-decoration-break:clone] [-webkit-box-decoration-break:clone]">{{ store.currentCue.text }}</span></div>
      <div class="absolute bottom-1 right-2 text-[10px] text-white/60 bg-black/40 px-1.5 rounded">
        {{ preset.label }} {{ preset.sub }} · {{ txSelected ? 'drag to place title · scroll to resize' : ovSelected ? 'drag to place overlay · scroll to resize' : 'drag to reposition · scroll to zoom' }}
      </div>
    </div>
    <audio
      v-for="a in store.activeAudioClips" :key="a.clip.id" :ref="(el) => setAudioEl(a.clip.id, el)" :src="api.assetUrl(a.clip.source)" preload="auto" data-testid="audio-clip"
      @error="onMediaError(a.clip.source)"
    />
  </div>
</template>
