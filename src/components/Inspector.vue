<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { useProjectStore } from "../stores/project";
import MuteToggle from "./MuteToggle.vue";
import { ASPECT_PRESETS, PLACEMENT_BADGE, PLACEMENT_FULL, TEXT_SIZE_MAX, TEXT_SIZE_MIN, clipDuration, clipName, type TextStyle, type Transition, scaleToZoom, zoomToScale } from "../types/project";
import { fmtMs } from "../utils/time";

const store = useProjectStore();
const clip = computed(() => store.selectedClip);
const isLast = computed(() => store.selectedIndex === store.clips.length - 1);
const ov = computed(() => store.selectedOverlay);
const au = computed(() => store.selectedAudio);
const cap = computed(() => store.selectedCaptions);
/** Inline caption editing in the list below; Enter / blur commit, Escape cancels, blank removes the cue. */
const editingCue = ref<{ id: string; text: string } | null>(null);
function editCue(c: { id: string; text: string }) { editingCue.value = { id: c.id, text: c.text }; }
function commitCue(id: string, before: string) {
  const d = editingCue.value; if (d?.id !== id) return; // Escape already closed it
  editingCue.value = null;
  const text = d.text.trim();
  if (text !== before) void store.setCueText(id, text);
}
const vFocus = { mounted: (el: HTMLInputElement) => { el.focus(); el.select(); } };
const tx = computed(() => store.selectedText);

// Debounced slider commits: sliders update a local copy, commit on change (pointer release).
const fades = ref({ fi: 0, fo: 0 });
const vol = ref({ v: 1, muted: false });
const trans = ref<{ type: Transition["type"]; ms: number }>({ type: "None", ms: 500 });
watch(clip, (c) => {
  if (!c) return;
  fades.value = { fi: c.fade_in, fo: c.fade_out };
  vol.value = { v: c.volume, muted: c.muted };
  trans.value = { type: c.transition_out.type, ms: c.transition_out.type === "None" ? trans.value.ms : c.transition_out.ms };
}, { immediate: true, deep: true });

const commitFades = () => clip.value && store.setFades(clip.value.id, fades.value.fi, fades.value.fo);
const commitVolume = () => clip.value && store.setVolume(clip.value.id, vol.value.v, vol.value.muted);
const commitTransition = () => {
  if (!clip.value) return;
  const t: Transition = trans.value.type === "None" ? { type: "None" } : { type: trans.value.type, ms: trans.value.ms };
  void store.setTransition(clip.value.id, t);
};

const maxFade = computed(() => (clip.value ? Math.min(5000, clipDuration(clip.value)) : 0));

// Overlay clip: fades, length (stills), size.
const o = ref({ fi: 0, fo: 0, len: 5000, scale: 0.35, opacity: 1 });
watch(ov, (c) => { if (c) o.value = { fi: c.fade_in, fo: c.fade_out, len: clipDuration(c), scale: c.placement.scale, opacity: c.opacity ?? 1 }; }, { immediate: true, deep: true });
const commitOvOpacity = () => ov.value && store.overlaySetOpacity(ov.value.id, o.value.opacity);
const ovMaxFade = computed(() => (ov.value ? Math.min(5000, clipDuration(ov.value)) : 0));
const commitOvFades = () => ov.value && store.overlaySetFades(ov.value.id, o.value.fi, o.value.fo);
const commitOvLen = () => ov.value && store.overlayTrim(ov.value.id, 0, Math.max(100, Math.round(o.value.len)));
const commitOvScale = () => ov.value && store.overlaySetPlacement(ov.value.id, { ...ov.value.placement, scale: o.value.scale });
const resetPlacement = () => ov.value && store.overlaySetPlacement(ov.value.id, ov.value.media.is_still ? { ...PLACEMENT_BADGE } : { ...PLACEMENT_FULL });
// Overlay sound: volume + mute, like a V1 clip; only for video files that carry audio.
const ovAudio = ref({ v: 1, muted: false });
watch(ov, (c) => { if (c) ovAudio.value = { v: c.volume, muted: c.muted }; }, { immediate: true, deep: true });
const ovHasAudio = computed(() => !!ov.value && ov.value.media.has_audio && !ov.value.media.is_still);
const commitOvAudio = () => ov.value && store.overlaySetAudio(ov.value.id, ovAudio.value.v, ovAudio.value.muted);

// Audio clip: volume, fades, mute; plus its track's label/mute.
const a = ref({ v: 1, fi: 0, fo: 0, muted: false });
watch(au, (x) => { if (x) a.value = { v: x.clip.volume, fi: x.clip.fade_in, fo: x.clip.fade_out, muted: x.clip.muted }; }, { immediate: true, deep: true });
const auMaxFade = computed(() => (au.value ? Math.min(10_000, clipDuration(au.value.clip)) : 0));
const commitAudio = () => au.value && store.audioClipSet(au.value.clip.id, a.value.v, a.value.fi, a.value.fo, a.value.muted);

// Title: the whole style is edited locally and sent in one piece; length, fades and position separately.
const t = ref<{ style: TextStyle; len: number; fi: number; fo: number; backdrop: boolean; bdColor: string; bdOpacity: number }>({
  style: { text: "", font: "Quicksand", size: 0.08, color: "#ffffff", backdrop: null }, len: 5000, fi: 0, fo: 0, backdrop: false, bdColor: "#000000", bdOpacity: 0.65,
});
watch(tx, (c) => {
  if (!c) return;
  t.value = {
    style: { ...c.style }, len: c.duration, fi: c.fade_in, fo: c.fade_out,
    backdrop: !!c.style.backdrop, bdColor: c.style.backdrop?.color ?? t.value.bdColor, bdOpacity: c.style.backdrop?.opacity ?? t.value.bdOpacity,
  };
}, { immediate: true, deep: true });
onMounted(() => { if (tx.value) void store.loadFonts(); });
watch(tx, (c) => { if (c) void store.loadFonts(); });
/** Every style control funnels through here so Rust always sees the complete style. */
function commitStyle() {
  if (!tx.value) return;
  const style: TextStyle = { ...t.value.style, backdrop: t.value.backdrop ? { color: t.value.bdColor, opacity: t.value.bdOpacity } : null };
  void store.textSetStyle(tx.value.id, style);
}
const commitTxLen = () => tx.value && store.textTrim(tx.value.id, Math.max(100, Math.round(t.value.len)));
const txMaxFade = computed(() => (tx.value ? Math.min(5000, tx.value.duration) : 0));
const commitTxFades = () => tx.value && store.textSetFades(tx.value.id, t.value.fi, t.value.fo);
const resetTxPosition = () => tx.value && store.textSetPosition(tx.value.id, 0.5, 0.85);
/** The picker lists what `fc-list` found; a font from a file made elsewhere is kept as its own entry. */
const fontCss = (f: string) => `"${f.replace(/"/g, "")}", sans-serif`;
const fontChoices = computed(() => (t.value.style.font && !store.fonts.includes(t.value.style.font) ? [t.value.style.font, ...store.fonts] : store.fonts));

const crop = computed(() => store.project?.crop ?? { scale: 1, x: 0.5, y: 0.5 });
/** Zoom slider position, -100 … 100 (0 = fills the frame); see zoomToScale. */
const cropZoom = ref(0);
watch(crop, (c) => { cropZoom.value = Math.round(scaleToZoom(c.scale)); }, { immediate: true });
const cropScale = computed(() => zoomToScale(cropZoom.value));
</script>

<template>
  <aside class="shrink-0 border-l border-line bg-panel overflow-y-auto text-xs" data-testid="inspector">
    <!-- Output -->
    <section class="p-3 border-b border-line">
      <h3 class="uppercase tracking-wide text-[10px] text-muted mb-2">Output</h3>
      <div class="grid grid-cols-2 gap-1.5">
        <button
          v-for="p in ASPECT_PRESETS" :key="p.id"
          class="rounded border px-2 py-1.5 text-left hover:border-muted"
          :class="store.project?.aspect === p.id ? 'border-accent bg-accent/10' : 'border-line'"
          @click="store.setAspect(p.id)"
        >
          <div class="font-medium">{{ p.label }}</div>
          <div class="text-muted">{{ p.sub }} · {{ p.w }}×{{ p.h }}</div>
        </button>
      </div>
      <label class="flex items-center gap-2 mt-3">
        <span class="w-14 text-muted">Zoom</span>
        <input
          type="range" min="-100" max="100" step="1" v-model.number="cropZoom" class="flex-1" data-testid="crop-zoom" list="zoom-stops"
          :title="`${cropZoom > 0 ? '+' : ''}${cropZoom} · ${cropScale.toFixed(2)}×`" @change="store.setCrop({ ...crop, scale: Number(cropScale.toFixed(4)) })"
        />
        <datalist id="zoom-stops"><option value="0" /></datalist>
        <span class="w-10 text-right font-mono" data-testid="crop-zoom-label">{{ cropZoom > 0 ? '+' : '' }}{{ cropZoom }}</span>
      </label>
      <div class="flex justify-between text-[10px] text-muted/70 px-16"><span>−100 · out</span><span>0</span><span>+100 · in</span></div>
      <button class="mt-1 text-muted hover:text-fg" @click="store.setCrop({ scale: 1, x: 0.5, y: 0.5 })">Reset framing</button>
    </section>

    <!-- Clip -->
    <section class="p-3 border-b border-line">
      <h3 class="uppercase tracking-wide text-[10px] text-muted mb-2">Clip</h3>
      <div v-if="!clip" class="text-muted">{{ cap ? 'Captions track selected.' : 'Select a clip on the timeline.' }}</div>
      <template v-else>
        <div class="font-medium truncate" :title="clip.source">{{ clipName(clip) }}</div>
        <div class="text-muted mb-2">
          {{ clip.media.width }}×{{ clip.media.height }} · {{ (clip.media.fps.num / clip.media.fps.den).toFixed(2) }} fps · {{ clip.media.codec }}
          <br />In {{ fmtMs(clip.source_start) }} · Out {{ fmtMs(clip.source_end) }} · {{ fmtMs(clipDuration(clip)) }}
        </div>

        <label class="flex items-center gap-2 mt-2">
          <span class="w-14 text-muted">Fade in</span>
          <input type="range" min="0" :max="maxFade" step="50" v-model.number="fades.fi" class="flex-1" @change="commitFades" />
          <span class="w-10 text-right font-mono">{{ (fades.fi / 1000).toFixed(2) }}s</span>
        </label>
        <label class="flex items-center gap-2 mt-1">
          <span class="w-14 text-muted">Fade out</span>
          <input type="range" min="0" :max="maxFade" step="50" v-model.number="fades.fo" class="flex-1" @change="commitFades" />
          <span class="w-10 text-right font-mono">{{ (fades.fo / 1000).toFixed(2) }}s</span>
        </label>

        <div class="mt-3 text-muted">Transition to next clip</div>
        <div class="flex gap-1 mt-1">
          <button v-for="t in (['None', 'CrossDissolve', 'DipToBlack'] as const)" :key="t" :disabled="isLast"
            class="flex-1 rounded border px-1 py-1" :class="trans.type === t ? 'border-accent bg-accent/10' : 'border-line'"
            @click="trans.type = t; commitTransition()">
            {{ t === 'None' ? 'Cut' : t === 'CrossDissolve' ? 'Dissolve' : 'Dip black' }}
          </button>
        </div>
        <label v-if="trans.type !== 'None'" class="flex items-center gap-2 mt-1">
          <span class="w-14 text-muted">Length</span>
          <input type="range" min="100" max="3000" step="50" v-model.number="trans.ms" class="flex-1" @change="commitTransition" />
          <span class="w-10 text-right font-mono">{{ (trans.ms / 1000).toFixed(2) }}s</span>
        </label>
        <div v-if="isLast" class="text-muted/70 mt-1">Last clip has nothing to transition into.</div>

        <label class="flex items-center gap-2 mt-3">
          <span class="w-14 text-muted">Volume</span>
          <input type="range" min="0" max="2" step="0.05" v-model.number="vol.v" class="flex-1" :disabled="!clip.media.has_audio" @change="commitVolume" />
          <span class="w-10 text-right font-mono">{{ Math.round(vol.v * 100) }}%</span>
        </label>
        <div class="flex items-center gap-2 mt-1" :class="!clip.media.has_audio ? 'opacity-40' : ''">
          <MuteToggle :muted="vol.muted" label="clip audio" :disabled="!clip.media.has_audio" @toggle="vol.muted = !vol.muted; commitVolume()" />
          <span class="text-muted">{{ vol.muted ? 'Clip audio muted' : 'Clip audio on' }}</span>
        </div>

        <div class="flex gap-1 mt-3">
          <button class="flex-1 rounded border border-line px-2 py-1 hover:border-muted" :disabled="!store.current || store.current.clip.id !== clip.id" @click="store.splitAtPlayhead()">Split at playhead (⌘T)</button>
          <button class="rounded border border-line px-2 py-1 text-danger hover:border-danger" @click="store.deleteClip(clip.id)">Delete</button>
        </div>
        <button
          v-if="store.selectedAll.length >= 2" class="w-full rounded border border-line px-2 py-1 mt-1 hover:border-muted" data-testid="join"
          title="Join the selected pieces of one file back into a single clip" @click="store.mergeSelected()"
        >Join {{ store.selectedAll.length }} clips (⌘J)</button>
      </template>
    </section>

    <!-- Overlay clip (V2) -->
    <section v-if="ov" class="p-3 border-b border-line">
      <h3 class="uppercase tracking-wide text-[10px] text-muted mb-2">Overlay</h3>
      <div class="font-medium truncate" :title="ov.source">{{ clipName(ov) }}</div>
      <div class="text-muted mb-2">
        {{ ov.media.is_still ? 'Still image' : ov.media.has_audio ? 'Video' : 'Video (no sound)' }} · {{ ov.media.width }}×{{ ov.media.height }}
        <br />At {{ fmtMs(ov.timeline_start) }} · {{ fmtMs(clipDuration(ov)) }}
      </div>
      <label v-if="ov.media.is_still" class="flex items-center gap-2 mt-2">
        <span class="w-14 text-muted">Length</span>
        <input type="range" min="100" :max="Math.max(30000, o.len)" step="100" v-model.number="o.len" class="flex-1" @change="commitOvLen" />
        <span class="w-10 text-right font-mono">{{ (o.len / 1000).toFixed(1) }}s</span>
      </label>
      <label class="flex items-center gap-2 mt-1">
        <span class="w-14 text-muted">Size</span>
        <input type="range" min="0.05" max="1" step="0.01" v-model.number="o.scale" class="flex-1" @change="commitOvScale" />
        <span class="w-10 text-right font-mono">{{ Math.round(o.scale * 100) }}%</span>
      </label>
      <label class="flex items-center gap-2 mt-1">
        <span class="w-14 text-muted">Opacity</span>
        <input type="range" min="0" max="1" step="0.01" v-model.number="o.opacity" class="flex-1" data-testid="overlay-opacity" @change="commitOvOpacity" />
        <span class="w-10 text-right font-mono">{{ Math.round(o.opacity * 100) }}%</span>
      </label>
      <button class="mt-1 text-muted hover:text-fg" @click="resetPlacement">Reset placement</button>
      <label class="flex items-center gap-2 mt-2">
        <span class="w-14 text-muted">Fade in</span>
        <input type="range" min="0" :max="ovMaxFade" step="50" v-model.number="o.fi" class="flex-1" @change="commitOvFades" />
        <span class="w-10 text-right font-mono">{{ (o.fi / 1000).toFixed(2) }}s</span>
      </label>
      <label class="flex items-center gap-2 mt-1">
        <span class="w-14 text-muted">Fade out</span>
        <input type="range" min="0" :max="ovMaxFade" step="50" v-model.number="o.fo" class="flex-1" @change="commitOvFades" />
        <span class="w-10 text-right font-mono">{{ (o.fo / 1000).toFixed(2) }}s</span>
      </label>
      <div class="text-muted/70 mt-1">Drag it in the preview to place · scroll to resize.</div>
      <template v-if="ovHasAudio">
        <label class="flex items-center gap-2 mt-2">
          <span class="w-14 text-muted">Volume</span>
          <input type="range" min="0" max="2" step="0.05" v-model.number="ovAudio.v" class="flex-1" data-testid="overlay-volume" @change="commitOvAudio" />
          <span class="w-10 text-right font-mono">{{ Math.round(ovAudio.v * 100) }}%</span>
        </label>
        <div class="flex items-center gap-2 mt-1">
          <MuteToggle :muted="ovAudio.muted" label="overlay" @toggle="ovAudio.muted = !ovAudio.muted; commitOvAudio()" />
          <span class="text-muted">{{ ovAudio.muted ? 'Clip muted' : 'Clip audio on' }} · row V{{ ov.layer + 2 }} {{ store.layerAudio(ov.layer).muted ? 'muted' : 'on' }}</span>
        </div>
      </template>
      <div class="flex gap-1 mt-3">
        <button class="flex-1 rounded border border-line px-2 py-1 hover:border-muted" @click="store.splitAtPlayhead()">Split at playhead (⌘T)</button>
        <button class="rounded border border-line px-2 py-1 text-danger hover:border-danger" @click="store.deleteSelected()">Delete</button>
      </div>
      <button
        v-if="store.selectedAll.length >= 2" class="w-full rounded border border-line px-2 py-1 mt-1 hover:border-muted" data-testid="join"
        title="Join the selected pieces of one file back into a single clip" @click="store.mergeSelected()"
        >Join {{ store.selectedAll.length }} clips (⌘J)</button>
    </section>

    <!-- Text track (T1) -->
    <section v-if="tx" class="p-3 border-b border-line" data-testid="text-section">
      <h3 class="uppercase tracking-wide text-[10px] text-muted mb-2">Title</h3>
      <div class="text-muted mb-2">At {{ fmtMs(tx.timeline_start) }} · {{ fmtMs(tx.duration) }} · row T{{ tx.layer + 1 }} · burned into the video above every track</div>
      <textarea
        v-model="t.style.text" rows="3" maxlength="500" data-testid="text-text" aria-label="Title text" placeholder="Title (Enter for a new line)"
        class="w-full bg-panel-2 border border-line rounded px-2 py-1 text-fg resize-y" @change="commitStyle"
      />
      <label class="flex items-center gap-2 mt-2">
        <span class="w-14 text-muted">Font</span>
        <select v-model="t.style.font" data-testid="text-font" class="flex-1 bg-panel-2 border border-line rounded px-1 py-0.5 text-fg" :style="{ fontFamily: fontCss(t.style.font) }" @change="commitStyle">
          <option v-for="f in fontChoices" :key="f" :value="f" :style="{ fontFamily: fontCss(f) }">{{ f }}</option>
        </select>
      </label>
      <label class="flex items-center gap-2 mt-1">
        <span class="w-14 text-muted">Size</span>
        <input type="range" :min="TEXT_SIZE_MIN" :max="TEXT_SIZE_MAX" step="0.005" v-model.number="t.style.size" class="flex-1" data-testid="text-size" @change="commitStyle" />
        <span class="w-10 text-right font-mono">{{ Math.round(t.style.size * 100) }}%</span>
      </label>
      <label class="flex items-center gap-2 mt-1">
        <span class="w-14 text-muted">Colour</span>
        <input type="color" v-model="t.style.color" data-testid="text-color" aria-label="Text colour" @change="commitStyle" />
        <span class="font-mono text-muted">{{ t.style.color }}</span>
      </label>
      <label class="flex items-center gap-2 mt-2">
        <input type="checkbox" v-model="t.backdrop" data-testid="text-backdrop" @change="commitStyle" />
        <span>Backdrop</span>
        <template v-if="t.backdrop">
          <input type="color" v-model="t.bdColor" data-testid="text-backdrop-color" aria-label="Backdrop colour" @change="commitStyle" />
          <input type="range" min="0" max="1" step="0.05" v-model.number="t.bdOpacity" class="flex-1" data-testid="text-backdrop-opacity" aria-label="Backdrop opacity" @change="commitStyle" />
          <span class="w-10 text-right font-mono">{{ Math.round(t.bdOpacity * 100) }}%</span>
        </template>
      </label>
      <div v-if="t.backdrop" class="text-muted/70 mt-1">A rounded rectangle behind the text, any colour.</div>
      <label class="flex items-center gap-2 mt-2">
        <span class="w-14 text-muted">Length</span>
        <input type="range" min="100" :max="Math.max(30000, t.len)" step="100" v-model.number="t.len" class="flex-1" data-testid="text-length" @change="commitTxLen" />
        <span class="w-10 text-right font-mono">{{ (t.len / 1000).toFixed(1) }}s</span>
      </label>
      <label class="flex items-center gap-2 mt-1">
        <span class="w-14 text-muted">Fade in</span>
        <input type="range" min="0" :max="txMaxFade" step="50" v-model.number="t.fi" class="flex-1" @change="commitTxFades" />
        <span class="w-10 text-right font-mono">{{ (t.fi / 1000).toFixed(2) }}s</span>
      </label>
      <label class="flex items-center gap-2 mt-1">
        <span class="w-14 text-muted">Fade out</span>
        <input type="range" min="0" :max="txMaxFade" step="50" v-model.number="t.fo" class="flex-1" @change="commitTxFades" />
        <span class="w-10 text-right font-mono">{{ (t.fo / 1000).toFixed(2) }}s</span>
      </label>
      <button class="mt-1 text-muted hover:text-fg" @click="resetTxPosition">Reset position</button>
      <div class="text-muted/70 mt-1">Drag it in the preview to place · scroll to resize.</div>
      <div class="flex gap-1 mt-3">
        <button class="flex-1 rounded border border-line px-2 py-1 hover:border-muted" @click="store.splitAtPlayhead()">Split at playhead (⌘T)</button>
        <button class="rounded border border-line px-2 py-1 text-danger hover:border-danger" @click="store.deleteSelected()">Delete</button>
      </div>
    </section>

    <!-- Captions track -->
    <section v-if="cap" class="p-3 border-b border-line" data-testid="captions-section">
      <h3 class="uppercase tracking-wide text-[10px] text-muted mb-2">Captions</h3>
      <div class="text-muted mb-2">{{ store.cues.length }} captions, derived from the transcript. They follow every edit to V1.</div>
      <label class="flex items-center gap-2">
        <input type="checkbox" :checked="store.captionsEnabled" data-testid="captions-enabled" @change="store.setCaptionsEnabled(($event.target as HTMLInputElement).checked)" />
        <span>{{ store.captionsEnabled ? 'Captions on' : 'Captions off' }}</span>
      </label>
      <div class="text-muted/70 mt-1">{{ store.captionsEnabled ? 'Shown in the preview; export writes an .srt beside the video.' : 'Hidden in the preview; export writes no .srt. The transcript is kept.' }}</div>
      <div class="text-muted/70 mt-1">Click a caption below (or double-click it on the timeline) to correct it; clear the text to remove it.</div>
      <ol v-if="store.cues.length" class="mt-2 max-h-72 overflow-y-auto divide-y divide-line/50 -mx-1" data-testid="cue-list">
        <li v-for="c in store.cues" :key="c.id" class="flex gap-2 px-1 py-1" :class="store.currentCue?.id === c.id ? 'bg-accent/10' : ''" data-testid="cue-row">
          <button class="font-mono text-muted shrink-0 hover:text-fg" :title="`Jump to ${fmtMs(c.start)}`" @click="store.playing = false; store.seek(c.start)">{{ fmtMs(c.start, false) }}</button>
          <input
            v-if="editingCue?.id === c.id" v-model="editingCue.text" v-focus data-testid="cue-edit" aria-label="Caption text"
            class="flex-1 min-w-0 bg-panel-2 border border-accent rounded px-1 text-fg"
            @keydown.stop @keydown.enter="commitCue(c.id, c.text)" @keydown.escape="editingCue = null" @blur="commitCue(c.id, c.text)"
          />
          <button v-else class="flex-1 min-w-0 text-left truncate hover:text-accent" :title="c.text" @click="editCue(c)">{{ c.text }}</button>
        </li>
      </ol>
    </section>

    <!-- Audio clip -->
    <section v-if="au" class="p-3 border-b border-line">
      <h3 class="uppercase tracking-wide text-[10px] text-muted mb-2">Audio · {{ au.track.label }}</h3>
      <div class="font-medium truncate" :title="au.clip.source">{{ clipName(au.clip) }}</div>
      <div class="text-muted mb-2">At {{ fmtMs(au.clip.timeline_start) }} · In {{ fmtMs(au.clip.source_start) }} · Out {{ fmtMs(au.clip.source_end) }} · {{ fmtMs(clipDuration(au.clip)) }}</div>
      <label class="flex items-center gap-2 mt-2">
        <span class="w-14 text-muted">Volume</span>
        <input type="range" min="0" max="2" step="0.05" v-model.number="a.v" class="flex-1" @change="commitAudio" />
        <span class="w-10 text-right font-mono">{{ Math.round(a.v * 100) }}%</span>
      </label>
      <label class="flex items-center gap-2 mt-1">
        <span class="w-14 text-muted">Fade in</span>
        <input type="range" min="0" :max="auMaxFade" step="100" v-model.number="a.fi" class="flex-1" @change="commitAudio" />
        <span class="w-10 text-right font-mono">{{ (a.fi / 1000).toFixed(1) }}s</span>
      </label>
      <label class="flex items-center gap-2 mt-1">
        <span class="w-14 text-muted">Fade out</span>
        <input type="range" min="0" :max="auMaxFade" step="100" v-model.number="a.fo" class="flex-1" @change="commitAudio" />
        <span class="w-10 text-right font-mono">{{ (a.fo / 1000).toFixed(1) }}s</span>
      </label>
      <div class="flex items-center gap-2 mt-1">
        <MuteToggle :muted="a.muted" label="clip" @toggle="a.muted = !a.muted; commitAudio()" />
        <span class="text-muted">{{ a.muted ? 'Clip muted' : 'Clip audio on' }}</span>
      </div>
      <div class="flex items-center gap-2 mt-1">
        <MuteToggle :muted="au.track.muted" label="track" @toggle="store.audioTrackUpdate(au.track.id, au.track.label, !au.track.muted, au.track.volume)" />
        <span class="text-muted">Track “{{ au.track.label }}” {{ au.track.muted ? 'muted' : 'on' }}</span>
      </div>
      <div class="text-muted/70 mt-1">Audio is trimmed to the video length on export.</div>
      <div class="flex gap-1 mt-3">
        <button class="flex-1 rounded border border-line px-2 py-1 hover:border-muted" @click="store.splitAtPlayhead()">Split at playhead (⌘T)</button>
        <button class="rounded border border-line px-2 py-1 text-danger hover:border-danger" @click="store.deleteSelected()">Delete</button>
      </div>
      <button
        v-if="store.selectedAll.length >= 2" class="w-full rounded border border-line px-2 py-1 mt-1 hover:border-muted" data-testid="join"
        title="Join the selected pieces of one file back into a single clip" @click="store.mergeSelected()"
        >Join {{ store.selectedAll.length }} clips (⌘J)</button>
    </section>

    <section v-if="!ov && !au" class="p-3 text-muted/70">
      Overlays (V2) and audio clips show their controls here when selected. Rename tracks in the timeline gutter.
    </section>
  </aside>
</template>
