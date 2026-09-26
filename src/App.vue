<script setup lang="ts">
import logo from "./assets/logo.svg";
import { onBeforeUnmount, onMounted, ref } from "vue";
import { open, save, ask } from "@tauri-apps/plugin-dialog";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useProjectStore } from "./stores/project";
import { api } from "./api/tauri";
import Preview from "./components/Preview.vue";
import Timeline from "./components/Timeline.vue";
import Inspector from "./components/Inspector.vue";
import ExportDialog from "./components/ExportDialog.vue";
import HelpDialog from "./components/HelpDialog.vue";

const store = useProjectStore();
const exportOpen = ref(false);
const helpOpen = ref(false);
const ffmpegMissing = ref(false);
const VIDEO_EXT = ["mp4", "mov", "m4v", "mkv", "webm", "avi", "mts", "m2ts"];
const AUDIO_EXT = ["mp3", "m4a", "aac", "wav", "aiff", "flac"];
const IMAGE_EXT = ["png", "jpg", "jpeg", "webp"];
const MEDIA_EXT = [...VIDEO_EXT, ...AUDIO_EXT, ...IMAGE_EXT];

/** ⌘I: video lands on V1 and in the pool; audio and images go to the pool. */
async function importDialog() {
  const p = await open({ multiple: true, filters: [{ name: "Media", extensions: MEDIA_EXT }, { name: "Video", extensions: VIDEO_EXT }] });
  if (Array.isArray(p)) await store.importMedia(p);
  else if (typeof p === "string") await store.importMedia([p]);
}

// Resizable timeline panel: drag the handle above it.
let resizeStart: { y: number; h: number } | null = null;
function startResize(e: PointerEvent) {
  resizeStart = { y: e.clientY, h: store.timelineHeight };
  (e.target as HTMLElement).setPointerCapture(e.pointerId);
}
function onResize(e: PointerEvent) {
  if (!resizeStart) return;
  store.timelineHeight = Math.round(Math.min(Math.max(200, resizeStart.h - (e.clientY - resizeStart.y)), window.innerHeight * 0.7));
}
function endResize() { resizeStart = null; }
async function openProject() {
  if (store.dirty && !(await ask("Discard unsaved changes?", { title: "ForgeVideo", kind: "warning" }))) return;
  const p = await open({ multiple: false, filters: [{ name: "ForgeVideo project", extensions: ["forgevideo", "json"] }] });
  if (typeof p === "string") await store.open(p);
}
async function saveProject(as = false) {
  let path: string | undefined;
  if (as || !(await api.projectSave().catch(() => null))) {
    const p = await save({ defaultPath: `${store.project?.name ?? "Untitled"}.forgevideo`, filters: [{ name: "ForgeVideo project", extensions: ["forgevideo"] }] });
    if (!p) return;
    path = p;
  }
  await store.save(path);
}
async function newProject() {
  if (store.dirty && !(await ask("Discard unsaved changes?", { title: "ForgeVideo", kind: "warning" }))) return;
  await store.newProject();
}

function onKey(e: KeyboardEvent) {
  const tag = (e.target as HTMLElement).tagName;
  if (tag === "INPUT" || tag === "TEXTAREA") return;
  const meta = e.metaKey || e.ctrlKey;
  if (e.key === "Escape") { helpOpen.value = false; return; }
  if (e.key === "?" && !meta) { e.preventDefault(); helpOpen.value = !helpOpen.value; return; }
  if (helpOpen.value) return;
  if (e.code === "Space") { e.preventDefault(); if (store.clips.length) store.playing = !store.playing; }
  else if (meta && e.key === "t") { e.preventDefault(); void store.splitAtPlayhead(); }
  else if ((e.key === "Backspace" || e.key === "Delete") && store.selected) { e.preventDefault(); void store.deleteSelected(); }
  else if (e.key === "ArrowLeft") { store.playing = false; store.seek(store.playhead - (e.shiftKey ? 1000 : 33)); }
  else if (e.key === "ArrowRight") { store.playing = false; store.seek(store.playhead + (e.shiftKey ? 1000 : 33)); }
  else if (e.key === "Home") { store.playing = false; store.seek(0); }
  else if (e.key === "End") { store.playing = false; store.seek(store.duration); }
  else if (meta && e.key === "s") { e.preventDefault(); void saveProject(e.shiftKey); }
  else if (meta && e.key === "o") { e.preventDefault(); void openProject(); }
  else if (meta && e.key === "i") { e.preventDefault(); void importDialog(); }
  else if (meta && e.key === "e") { e.preventDefault(); exportOpen.value = true; }
}

let unlistenDrop: (() => void) | undefined;
onMounted(async () => {
  await store.load();
  const st = await api.ffmpegStatus();
  ffmpegMissing.value = !st.ffmpeg || !st.ffprobe;
  window.addEventListener("keydown", onKey);
  unlistenDrop = await getCurrentWebview().onDragDropEvent((e) => {
    if (e.payload.type !== "drop") return;
    const paths = e.payload.paths.filter((p) => MEDIA_EXT.includes(p.split(".").pop()?.toLowerCase() ?? ""));
    if (paths.length) void store.poolAdd(paths);
  });
  void getCurrentWindow().setTitle("ForgeVideo");
});
onBeforeUnmount(() => { window.removeEventListener("keydown", onKey); unlistenDrop?.(); });
</script>

<template>
  <div class="h-full flex flex-col">
    <header class="h-11 shrink-0 flex items-center gap-1 px-3 border-b border-line bg-panel text-xs">
      <img :src="logo" alt="" class="h-7 w-auto mr-1.5 select-none" draggable="false" />
      <span class="font-display font-bold text-base mr-3 tracking-tight text-accent">ForgeVideo</span>
      <button class="px-2 py-1 rounded hover:bg-panel-2" @click="newProject">New</button>
      <button class="px-2 py-1 rounded hover:bg-panel-2" @click="openProject">Open</button>
      <button class="px-2 py-1 rounded hover:bg-panel-2" @click="saveProject(false)">Save{{ store.dirty ? ' •' : '' }}</button>
      <span class="w-px h-5 bg-line mx-1" />
      <button class="px-2 py-1 rounded hover:bg-panel-2" @click="importDialog">Import…</button>
      <span class="flex-1" />
      <span v-if="store.error" class="text-danger truncate max-w-md mr-3" :title="store.error">{{ store.error }}</span>
      <span v-if="ffmpegMissing" class="text-danger mr-3">ffmpeg not found — brew install ffmpeg</span>
      <button class="px-2 py-1 rounded hover:bg-panel-2" :disabled="!store.clips.length" @click="store.playing = !store.playing">{{ store.playing ? '⏸ Pause' : '▶ Play' }}</button>
      <button class="ml-1 px-3 py-1.5 rounded bg-accent text-black font-medium disabled:opacity-40" :disabled="!store.clips.length" @click="exportOpen = true">Export…</button>
      <button class="ml-1 w-7 h-7 rounded-full border border-line hover:bg-panel-2 font-semibold" title="Keyboard shortcuts (?)" aria-label="Help: keyboard shortcuts" @click="helpOpen = true">?</button>
    </header>

    <div class="flex-1 min-h-0 flex">
      <Preview />
      <Inspector />
    </div>

    <div
      class="h-1 shrink-0 cursor-row-resize bg-line hover:bg-accent/60" title="Drag to resize the timeline" data-testid="timeline-resize"
      @pointerdown="startResize" @pointermove="onResize" @pointerup="endResize" @pointercancel="endResize"
    />
    <div class="shrink-0" :style="{ height: store.timelineHeight + 'px' }">
      <Timeline />
    </div>

    <ExportDialog :open="exportOpen" @close="exportOpen = false" />
    <HelpDialog :open="helpOpen" @close="helpOpen = false" />
  </div>
</template>
