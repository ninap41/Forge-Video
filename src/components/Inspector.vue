<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { useProjectStore } from "../stores/project";
import MuteToggle from "./MuteToggle.vue";
import { ASPECT_PRESETS, PLACEMENT_BADGE, PLACEMENT_FULL, clipDuration, clipName, type Transition } from "../types/project";
import { fmtMs } from "../utils/time";

const store = useProjectStore();
const clip = computed(() => store.selectedClip);
const isLast = computed(() => store.selectedIndex === store.clips.length - 1);
const ov = computed(() => store.selectedOverlay);
const au = computed(() => store.selectedAudio);

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
const o = ref({ fi: 0, fo: 0, len: 5000, scale: 0.35 });
watch(ov, (c) => { if (c) o.value = { fi: c.fade_in, fo: c.fade_out, len: clipDuration(c), scale: c.placement.scale }; }, { immediate: true, deep: true });
const ovMaxFade = computed(() => (ov.value ? Math.min(5000, clipDuration(ov.value)) : 0));
const commitOvFades = () => ov.value && store.overlaySetFades(ov.value.id, o.value.fi, o.value.fo);
const commitOvLen = () => ov.value && store.overlayTrim(ov.value.id, 0, Math.max(100, Math.round(o.value.len)));
const commitOvScale = () => ov.value && store.overlaySetPlacement(ov.value.id, { ...ov.value.placement, scale: o.value.scale });
const resetPlacement = () => ov.value && store.overlaySetPlacement(ov.value.id, ov.value.media.is_still ? { ...PLACEMENT_BADGE } : { ...PLACEMENT_FULL });

// Audio clip: volume, fades, mute; plus its track's label/mute.
const a = ref({ v: 1, fi: 0, fo: 0, muted: false });
watch(au, (x) => { if (x) a.value = { v: x.clip.volume, fi: x.clip.fade_in, fo: x.clip.fade_out, muted: x.clip.muted }; }, { immediate: true, deep: true });
const auMaxFade = computed(() => (au.value ? Math.min(10_000, clipDuration(au.value.clip)) : 0));
const commitAudio = () => au.value && store.audioClipSet(au.value.clip.id, a.value.v, a.value.fi, a.value.fo, a.value.muted);

const crop = computed(() => store.project?.crop ?? { scale: 1, x: 0.5, y: 0.5 });
const cropScale = ref(1);
watch(crop, (c) => { cropScale.value = c.scale; }, { immediate: true });
</script>

<template>
  <aside class="w-72 shrink-0 border-l border-line bg-panel overflow-y-auto text-xs">
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
        <input type="range" min="1" max="4" step="0.01" v-model.number="cropScale" class="flex-1" @change="store.setCrop({ ...crop, scale: cropScale })" />
        <span class="w-10 text-right font-mono">{{ cropScale.toFixed(2) }}×</span>
      </label>
      <button class="mt-1 text-muted hover:text-fg" @click="store.setCrop({ scale: 1, x: 0.5, y: 0.5 })">Reset framing</button>
    </section>

    <!-- Clip -->
    <section class="p-3 border-b border-line">
      <h3 class="uppercase tracking-wide text-[10px] text-muted mb-2">Clip</h3>
      <div v-if="!clip" class="text-muted">Select a clip on the timeline.</div>
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
      </template>
    </section>

    <!-- Overlay clip (V2) -->
    <section v-if="ov" class="p-3 border-b border-line">
      <h3 class="uppercase tracking-wide text-[10px] text-muted mb-2">Overlay</h3>
      <div class="font-medium truncate" :title="ov.source">{{ clipName(ov) }}</div>
      <div class="text-muted mb-2">
        {{ ov.media.is_still ? 'Still image' : 'Video (silent)' }} · {{ ov.media.width }}×{{ ov.media.height }}
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
      <div class="flex gap-1 mt-3">
        <button class="flex-1 rounded border border-line px-2 py-1 hover:border-muted" @click="store.splitAtPlayhead()">Split at playhead (⌘T)</button>
        <button class="rounded border border-line px-2 py-1 text-danger hover:border-danger" @click="store.deleteSelected()">Delete</button>
      </div>
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
    </section>

    <section v-if="!ov && !au" class="p-3 text-muted/70">
      Overlays (V2) and audio clips show their controls here when selected. Rename tracks in the timeline gutter.
    </section>
  </aside>
</template>
