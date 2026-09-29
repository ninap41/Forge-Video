<script setup lang="ts">
// Small transient banner for feedback that isn't an error: rejected drops, duplicate imports.
import { useProjectStore } from "../stores/project";
const store = useProjectStore();
</script>

<template>
  <Transition enter-active-class="transition-opacity duration-150" leave-active-class="transition-opacity duration-300" enter-from-class="opacity-0" leave-to-class="opacity-0">
    <div
      v-if="store.notice" role="status" data-testid="banner"
      class="fixed left-1/2 top-14 -translate-x-1/2 z-50 px-3 py-1.5 rounded-md bg-panel-2 border border-accent/60 text-fg text-xs shadow-lg cursor-pointer select-none"
      @click="store.notify(null)"
    >{{ store.notice }}</div>
  </Transition>
</template>
