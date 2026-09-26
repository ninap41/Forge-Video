<script setup lang="ts">
// Speaker icon: green when audio is on, struck through and grey when muted.
defineProps<{ muted: boolean; label?: string; size?: number; disabled?: boolean }>();
const emit = defineEmits<{ toggle: [] }>();
</script>

<template>
  <button
    type="button" class="inline-flex items-center justify-center rounded hover:bg-panel-2" :class="muted ? 'text-muted' : 'text-accent'"
    :title="muted ? `Unmute ${label ?? ''}`.trim() : `Mute ${label ?? ''}`.trim()" :aria-label="muted ? `Unmute ${label ?? ''}`.trim() : `Mute ${label ?? ''}`.trim()"
    :aria-pressed="!muted" :data-muted="muted" :disabled="disabled" @click.stop="!disabled && emit('toggle')"
  >
    <svg :width="size ?? 16" :height="size ?? 16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      <path d="M11 5 6 9H2v6h4l5 4V5z" fill="currentColor" stroke="none" />
      <template v-if="muted">
        <line x1="23" y1="9" x2="17" y2="15" />
        <line x1="17" y1="9" x2="23" y2="15" />
      </template>
      <template v-else>
        <path d="M15.5 8.5a5 5 0 0 1 0 7" />
        <path d="M19 5a9 9 0 0 1 0 14" />
      </template>
    </svg>
  </button>
</template>
