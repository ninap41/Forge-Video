<script setup lang="ts">
// A title on the text track (T1). Thin sibling of FreeBlock: no media, just a length and a look.
import { computed } from "vue";
import type { TextClip } from "../types/project";
import { clipName } from "../types/project";

const props = defineProps<{ clip: TextClip; pxPerMs: number; selected: boolean; dimmed?: boolean }>();
const emit = defineEmits<{ select: [extend: boolean]; trimStart: [e: PointerEvent]; trimEnd: [e: PointerEvent]; dragStart: [e: PointerEvent]; contextMenu: [e: MouseEvent] }>();

const width = computed(() => props.clip.duration * props.pxPerMs);
const fontCss = computed(() => `"${props.clip.style.font.replace(/"/g, "")}", sans-serif`);
</script>

<template>
  <div
    class="absolute top-0 h-full rounded-md overflow-hidden border select-none group bg-accent/10"
    :class="[selected ? 'border-accent ring-2 ring-accent/40 z-10' : 'border-line hover:border-muted', dimmed ? 'opacity-50' : '']"
    :style="{ left: clip.timeline_start * pxPerMs + 'px', width: width + 'px' }"
    :data-clip-id="clip.id" data-testid="text-block"
    @pointerdown.stop="emit('select', $event.shiftKey); emit('dragStart', $event)" @contextmenu.prevent.stop="selected || emit('select', $event.shiftKey); emit('contextMenu', $event)"
  >
    <div
      class="absolute left-1.5 top-1 text-[11px] font-medium px-1 rounded truncate max-w-[calc(100%-12px)]"
      :style="{ color: clip.style.color, backgroundColor: clip.style.backdrop ? clip.style.backdrop.color : 'rgba(0,0,0,0.4)', fontFamily: fontCss }"
    >T {{ clipName(clip) }}</div>
    <div v-if="clip.fade_in" class="absolute left-0 top-0 h-full bg-gradient-to-r from-black/80 to-transparent pointer-events-none" :style="{ width: clip.fade_in * pxPerMs + 'px' }" />
    <div v-if="clip.fade_out" class="absolute right-0 top-0 h-full bg-gradient-to-l from-black/80 to-transparent pointer-events-none" :style="{ width: clip.fade_out * pxPerMs + 'px' }" />
    <div class="absolute left-0 top-0 h-full w-2 cursor-ew-resize bg-accent/0 group-hover:bg-accent/60" @pointerdown.stop="emit('select', false); emit('trimStart', $event)" />
    <div class="absolute right-0 top-0 h-full w-2 cursor-ew-resize bg-accent/0 group-hover:bg-accent/60" @pointerdown.stop="emit('select', false); emit('trimEnd', $event)" />
  </div>
</template>
