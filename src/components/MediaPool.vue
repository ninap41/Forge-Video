<script setup lang="ts">
// Imported media that may or may not be on the timeline. Tabs by kind, grid or list, drag onto a track.
import { computed, onBeforeUnmount, ref } from "vue";
import { open } from "@tauri-apps/plugin-dialog";
import { useProjectStore } from "../stores/project";
import { api } from "../api/tauri";
import { mediaKind, type MediaKind, type PoolItem } from "../types/project";
import { basename, fmtMs } from "../utils/time";
import { AUDIO_EXT, IMAGE_EXT, MEDIA_EXT, VIDEO_EXT } from "../utils/media";

export type PoolTab = "All" | MediaKind;
const store = useProjectStore();
const tab = ref<PoolTab>("All");
const TABS: { id: PoolTab; label: string; ext: string[] }[] = [
  { id: "All", label: "All", ext: MEDIA_EXT },
  { id: "Video", label: "Clips", ext: VIDEO_EXT },
  { id: "Audio", label: "Audio", ext: [...AUDIO_EXT, ...VIDEO_EXT] }, // the audio of a video is fair game
  { id: "Image", label: "Images", ext: IMAGE_EXT },
];
const inTab = (i: PoolItem, t: PoolTab) => t === "All" || mediaKind(i.media) === t;
const items = computed(() => store.pool.filter((i) => inTab(i, tab.value)));
const count = (t: PoolTab) => store.pool.filter((i) => inTab(i, t)).length;

async function importFiles() {
  const t = TABS.find((t) => t.id === tab.value)!;
  const p = await open({ multiple: true, filters: [{ name: t.label, extensions: t.ext }] });
  const paths = Array.isArray(p) ? p : typeof p === "string" ? [p] : [];
  if (paths.length) await store.poolAdd(paths);
}

function thumbOf(i: PoolItem): string | null {
  if (i.media.is_still) return api.assetUrl(i.path);
  return store.thumbs[i.path]?.urls[0] ?? null;
}

/** Double-click appends video and images to V1; audio goes onto the first track at the playhead. */
async function place(i: PoolItem) {
  const k = mediaKind(i.media);
  if (k !== "Audio") await store.insertClip(i.path, store.clips.length);
  else {
    if (!store.audioTracks.length) await store.audioTrackAdd("Music");
    const t = store.audioTracks[0]; if (t) await store.audioClipAdd(t.id, i.path, store.playhead);
  }
}

// ---- pointer-driven drag onto the timeline (HTML5 DnD is unreliable inside Tauri's webview) ----
let pending: { item: PoolItem; x0: number; y0: number } | null = null;
function onPointerDown(i: PoolItem, e: PointerEvent) {
  if (e.button !== 0) return;
  pending = { item: i, x0: e.clientX, y0: e.clientY };
  window.addEventListener("pointermove", onPointerMove);
  window.addEventListener("pointerup", onPointerUp, { once: true });
}
function onPointerMove(e: PointerEvent) {
  if (!pending) return;
  if (!store.poolDrag && Math.hypot(e.clientX - pending.x0, e.clientY - pending.y0) < 4) return;
  store.poolDrag = { item: pending.item, x: e.clientX, y: e.clientY };
}
function onPointerUp(e: PointerEvent) {
  window.removeEventListener("pointermove", onPointerMove);
  const d = store.poolDrag; pending = null;
  if (!d) return;
  store.poolDrag = null;
  const under = document.elementFromPoint(e.clientX, e.clientY);
  const row = under?.closest("[data-row]");
  if (row) { row.dispatchEvent(new CustomEvent("pooldrop", { detail: { item: d.item, clientX: e.clientX }, bubbles: false })); return; }
  // Released inside the timeline but on no row: the only way that happens with audio is having no audio track yet.
  if (under?.closest("[data-testid=timeline-tracks]") && mediaKind(d.item.media) === "Audio" && !store.audioTracks.length) {
    store.notify("No audio track yet — click “+ Track”, then drop the audio there");
  }
}
onBeforeUnmount(() => window.removeEventListener("pointermove", onPointerMove));
</script>

<template>
  <div class="flex flex-col min-h-0 text-xs">
    <div class="flex items-center gap-1 px-2 h-7 shrink-0 border-b border-line">
      <span class="uppercase tracking-wide text-[10px] text-muted mr-2">Media pool</span>
      <button
        v-for="t in TABS" :key="t.id" class="px-2 py-0.5 rounded" :class="tab === t.id ? 'bg-panel-2 text-fg' : 'text-muted hover:text-fg'"
        @click="tab = t.id"
      >{{ t.label }} <span class="opacity-60">{{ count(t.id) }}</span></button>
      <span class="flex-1" />
      <button class="px-2 py-0.5 rounded border border-line hover:border-muted" @click="importFiles">Import…</button>
      <button class="w-6 text-center" :class="store.poolView === 'grid' ? 'text-fg' : 'text-muted hover:text-fg'" title="Thumbnails" aria-label="Thumbnail view" @click="store.poolView = 'grid'">▦</button>
      <button class="w-6 text-center" :class="store.poolView === 'list' ? 'text-fg' : 'text-muted hover:text-fg'" title="List" aria-label="List view" @click="store.poolView = 'list'">☰</button>
    </div>

    <div class="flex-1 min-h-0 overflow-y-auto p-2" data-testid="pool-items">
      <div v-if="!items.length" class="text-muted/70 pt-2 text-center">
        Nothing here yet · drop files anywhere in the window, or click Import…
      </div>
      <div v-else-if="store.poolView === 'grid'" class="grid gap-2" style="grid-template-columns: repeat(auto-fill, minmax(112px, 1fr))">
        <div
          v-for="i in items" :key="i.id" class="group relative rounded border border-line hover:border-muted bg-panel-2 cursor-grab select-none overflow-hidden"
          :title="i.path" @pointerdown="onPointerDown(i, $event)" @dblclick="place(i)"
        >
          <div class="h-16 bg-black/40 flex items-center justify-center overflow-hidden">
            <img v-if="thumbOf(i)" :src="thumbOf(i)!" class="max-h-full max-w-full object-contain pointer-events-none" draggable="false" />
            <span v-else class="text-2xl text-muted">{{ mediaKind(i.media) === 'Audio' ? '♪' : '▣' }}</span>
          </div>
          <div class="px-1.5 py-1 truncate">{{ basename(i.path) }}</div>
          <div class="px-1.5 pb-1 text-muted">{{ i.media.is_still ? `${i.media.width}×${i.media.height}` : fmtMs(i.media.duration_ms) }}</div>
          <button class="absolute top-0.5 right-0.5 hidden group-hover:block w-4 h-4 rounded bg-black/60 text-white/80 hover:text-danger" title="Remove from pool" aria-label="Remove from pool" @pointerdown.stop @click.stop="store.poolRemove(i.id)">✕</button>
        </div>
      </div>
      <table v-else class="w-full">
        <tbody>
          <tr
            v-for="i in items" :key="i.id" class="border-b border-line/50 hover:bg-panel-2 cursor-grab select-none"
            :title="i.path" @pointerdown="onPointerDown(i, $event)" @dblclick="place(i)"
          >
            <td class="py-1 pr-2 w-6 text-muted">{{ mediaKind(i.media) === 'Audio' ? '♪' : mediaKind(i.media) === 'Image' ? '▣' : '▶' }}</td>
            <td class="py-1 pr-2 truncate">{{ basename(i.path) }}</td>
            <td class="py-1 pr-2 text-muted font-mono w-16">{{ i.media.is_still ? '' : fmtMs(i.media.duration_ms) }}</td>
            <td class="py-1 pr-2 text-muted w-24">{{ i.media.width ? `${i.media.width}×${i.media.height}` : i.media.audio_codec }}</td>
            <td class="py-1 w-6"><button class="text-muted hover:text-danger" title="Remove from pool" aria-label="Remove from pool" @pointerdown.stop @click.stop="store.poolRemove(i.id)">✕</button></td>
          </tr>
        </tbody>
      </table>
    </div>

    <!-- drag ghost -->
    <div v-if="store.poolDrag" class="fixed z-50 pointer-events-none px-2 py-1 rounded bg-accent text-black text-[11px] shadow-lg" :style="{ left: store.poolDrag.x + 12 + 'px', top: store.poolDrag.y + 12 + 'px' }">
      {{ basename(store.poolDrag.item.path) }}
    </div>
  </div>
</template>
