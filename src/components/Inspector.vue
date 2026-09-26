<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { open } from "@tauri-apps/plugin-dialog";
import { useProjectStore } from "../stores/project";
import { ASPECT_PRESETS, clipDuration, type Transition } from "../types/project";
import { basename, fmtMs } from "../utils/time";

const store = useProjectStore();
const clip = computed(() => store.selectedClip);
const isLast = computed(() => store.selectedIndex === store.clips.length - 1);
const music = computed(() => store.project?.music ?? null);

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

const m = ref({ volume: 0.5, fade_in: 0, fade_out: 0, timeline_start: 0, muted: false });
watch(music, (t) => { if (t) m.value = { volume: t.volume, fade_in: t.fade_in, fade_out: t.fade_out, timeline_start: t.timeline_start, muted: t.muted }; }, { immediate: true, deep: true });
const commitMusic = () => music.value && store.updateMusic({ ...music.value, ...m.value });

async function pickMusic() {
  const p = await open({ multiple: false, filters: [{ name: "Audio", extensions: ["mp3", "m4a", "aac", "wav", "aiff", "flac", "mp4", "mov"] }] });
  if (typeof p === "string") await store.setMusic(p);
}

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
        <div class="font-medium truncate" :title="clip.source">{{ basename(clip.source) }}</div>
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
        <label class="flex items-center gap-2 mt-1 cursor-pointer">
          <input type="checkbox" v-model="vol.muted" :disabled="!clip.media.has_audio" @change="commitVolume" /> Mute clip audio
        </label>

        <div class="flex gap-1 mt-3">
          <button class="flex-1 rounded border border-line px-2 py-1 hover:border-muted" :disabled="!store.current || store.current.clip.id !== clip.id" @click="store.splitAtPlayhead()">Split at playhead (S)</button>
          <button class="rounded border border-line px-2 py-1 text-danger hover:border-danger" @click="store.deleteClip(clip.id)">Delete</button>
        </div>
      </template>
    </section>

    <!-- Music -->
    <section class="p-3">
      <h3 class="uppercase tracking-wide text-[10px] text-muted mb-2">Music</h3>
      <div v-if="!music">
        <button class="rounded border border-line px-2 py-1 hover:border-muted" @click="pickMusic">Add music track…</button>
      </div>
      <template v-else>
        <div class="flex items-center gap-2">
          <span class="font-medium truncate flex-1" :title="music.source">{{ basename(music.source) }}</span>
          <button class="text-muted hover:text-fg" @click="pickMusic">Change</button>
          <button class="text-danger" @click="store.setMusic(null)">Remove</button>
        </div>
        <label class="flex items-center gap-2 mt-2">
          <span class="w-14 text-muted">Volume</span>
          <input type="range" min="0" max="1.5" step="0.05" v-model.number="m.volume" class="flex-1" @change="commitMusic" />
          <span class="w-10 text-right font-mono">{{ Math.round(m.volume * 100) }}%</span>
        </label>
        <label class="flex items-center gap-2 mt-1">
          <span class="w-14 text-muted">Start at</span>
          <input type="range" min="0" :max="Math.max(0, store.duration)" step="100" v-model.number="m.timeline_start" class="flex-1" @change="commitMusic" />
          <span class="w-10 text-right font-mono">{{ fmtMs(m.timeline_start) }}</span>
        </label>
        <label class="flex items-center gap-2 mt-1">
          <span class="w-14 text-muted">Fade in</span>
          <input type="range" min="0" max="10000" step="100" v-model.number="m.fade_in" class="flex-1" @change="commitMusic" />
          <span class="w-10 text-right font-mono">{{ (m.fade_in / 1000).toFixed(1) }}s</span>
        </label>
        <label class="flex items-center gap-2 mt-1">
          <span class="w-14 text-muted">Fade out</span>
          <input type="range" min="0" max="10000" step="100" v-model.number="m.fade_out" class="flex-1" @change="commitMusic" />
          <span class="w-10 text-right font-mono">{{ (m.fade_out / 1000).toFixed(1) }}s</span>
        </label>
        <label class="flex items-center gap-2 mt-1 cursor-pointer"><input type="checkbox" v-model="m.muted" @change="commitMusic" /> Mute</label>
        <div class="text-muted/70 mt-1">Music is trimmed to the video length on export.</div>
      </template>
    </section>
  </aside>
</template>
