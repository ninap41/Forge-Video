<script lang="ts">
// Single source of truth for the hot-key list (also asserted by App.test.ts and mirrored in README.md).
export const SHORTCUTS: { keys: string; action: string }[] = [
  { keys: "Space", action: "Play / pause" },
  { keys: "S", action: "Split clip at playhead" },
  { keys: "⌫ / Delete", action: "Delete selected clip" },
  { keys: "← / →", action: "Nudge playhead one frame" },
  { keys: "⇧ ← / ⇧ →", action: "Nudge playhead one second" },
  { keys: "Home / End", action: "Jump to start / end" },
  { keys: "⌘ I", action: "Import video…" },
  { keys: "⌘ O", action: "Open project…" },
  { keys: "⌘ S", action: "Save project" },
  { keys: "⇧ ⌘ S", action: "Save project as…" },
  { keys: "⌘ E", action: "Export…" },
  { keys: "?", action: "Show this help" },
  { keys: "Esc", action: "Close dialog" },
];

export const MOUSE: { keys: string; action: string }[] = [
  { keys: "Drag clip edge", action: "Trim in / out point (frame-snapped)" },
  { keys: "Drag clip", action: "Reorder clips" },
  { keys: "Click ruler / drag", action: "Scrub the playhead" },
  { keys: "Drag preview", action: "Reposition the crop" },
  { keys: "Scroll on preview", action: "Zoom the crop" },
  { keys: "Drop video file", action: "Import" },
];
</script>

<script setup lang="ts">
defineProps<{ open: boolean }>();
const emit = defineEmits<{ close: [] }>();
</script>

<template>
  <div v-if="open" class="fixed inset-0 z-50 bg-black/60 flex items-center justify-center" @pointerdown.self="emit('close')">
    <div class="w-[460px] rounded-lg bg-panel border border-line shadow-2xl p-4 text-xs" role="dialog" aria-label="Keyboard shortcuts">
      <div class="flex items-center justify-between mb-3">
        <h2 class="text-sm font-semibold">Keyboard shortcuts</h2>
        <button class="text-muted hover:text-fg" aria-label="Close" @click="emit('close')">✕</button>
      </div>
      <table class="w-full">
        <tbody>
          <tr v-for="s in SHORTCUTS" :key="s.keys" class="border-b border-line/50 last:border-0">
            <td class="py-1 pr-3 w-32"><kbd class="font-mono rounded border border-line bg-panel-2 px-1.5 py-0.5">{{ s.keys }}</kbd></td>
            <td class="py-1 text-fg">{{ s.action }}</td>
          </tr>
        </tbody>
      </table>
      <h3 class="uppercase tracking-wide text-[10px] text-muted mt-4 mb-1">Mouse</h3>
      <table class="w-full">
        <tbody>
          <tr v-for="s in MOUSE" :key="s.keys" class="border-b border-line/50 last:border-0">
            <td class="py-1 pr-3 w-32 text-muted">{{ s.keys }}</td>
            <td class="py-1 text-fg">{{ s.action }}</td>
          </tr>
        </tbody>
      </table>
      <div class="flex justify-end mt-4">
        <button class="rounded bg-accent text-black font-medium px-3 py-1.5" @click="emit('close')">Close</button>
      </div>
    </div>
  </div>
</template>
