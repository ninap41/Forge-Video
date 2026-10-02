<script setup lang="ts">
import { useProjectStore } from "../stores/project";
import type { Loop } from "../types/project";
import { sameRange } from "../types/project";
import { fmtMs } from "../utils/time";

const store = useProjectStore();
const secs = (ms: number) => `${(ms / 1000).toFixed(ms % 1000 ? 1 : 0)} s`;
/** ❚❚ while this loop is the one playing on repeat. */
const looping = (l: Loop) => store.playing && store.loopOn && sameRange(store.range, l);
</script>

<template>
  <div class="flex flex-col min-h-0 border-b border-line text-xs" data-testid="loops-drawer">
    <div class="flex items-center gap-2 px-3 h-7 shrink-0">
      <button class="flex items-center gap-1 text-muted hover:text-fg" :aria-expanded="store.loopsOpen" data-testid="loops-toggle" @click="store.loopsOpen = !store.loopsOpen">
        <span class="inline-block w-3 text-center transition-transform" :class="store.loopsOpen ? 'rotate-90' : ''">▸</span>
        Pinned loops <span class="text-muted/70">({{ store.loops.length }})</span>
      </button>
      <span class="flex-1" />
      <button
        data-testid="pin-range" class="rounded border border-line px-1.5 py-0.5 hover:text-fg disabled:opacity-40" :disabled="!store.range || !!store.activeLoop"
        :title="store.range ? 'Keep the selected range as a loop' : 'Drag on the ruler to select a range first'" @click="store.loopAdd('')"
      >Pin range</button>
    </div>
    <div v-if="store.loopsOpen" class="px-3 pb-2 flex-1 min-h-0 overflow-y-auto">
      <div v-if="!store.loops.length" class="text-muted/60 py-1">Drag on the ruler to select a range, then Pin. Loops play on repeat and can be exported on their own.</div>
      <div
        v-for="l in store.loops" :key="l.id" data-testid="loop" class="flex items-center gap-2 h-6 rounded px-1 -mx-1"
        :class="store.activeLoopId === l.id ? 'bg-yellow-400/10 text-yellow-400' : 'text-fg'"
      >
        <button class="w-5 text-center hover:text-fg" :title="looping(l) ? 'Pause' : 'Play this loop on repeat'" :aria-label="`${looping(l) ? 'Pause' : 'Play'} ${l.name}`" data-testid="loop-play" @click="store.playRange(l, l.id)">{{ looping(l) ? '❚❚' : '▶' }}</button>
        <button class="flex-1 min-w-0 text-left truncate hover:underline" :title="`Show ${l.name} on the timeline`" data-testid="loop-name" @click="store.selectLoop(l.id)">{{ l.name }}</button>
        <span class="font-mono text-muted">{{ fmtMs(l.start, false) }}–{{ fmtMs(l.end, false) }} · {{ secs(l.end - l.start) }}</span>
        <button class="rounded border border-line px-1.5 hover:text-fg" title="Export just this loop" data-testid="loop-export" @click="store.exportLoop(l.id)">Export</button>
        <button class="w-4 text-center text-muted hover:text-danger" title="Unpin" :aria-label="`Unpin ${l.name}`" data-testid="loop-unpin" @click="store.loopRemove(l.id)">✕</button>
      </div>
    </div>
  </div>
</template>
