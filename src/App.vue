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
import AiPanel from "./components/AiPanel.vue";
import ExportDialog from "./components/ExportDialog.vue";
import HelpDialog, { SHORTCUTS_SECTION } from "./components/HelpDialog.vue";
import WelcomeDialog, { welcomeSeen } from "./components/WelcomeDialog.vue";
import Banner from "./components/Banner.vue";
import MediaPool from "./components/MediaPool.vue";
import LoopsDrawer from "./components/LoopsDrawer.vue";
import { MEDIA_EXT, VIDEO_EXT, isMediaPath } from "./utils/media";
import { basename } from "./utils/time";

const store = useProjectStore();
const helpOpen = ref(false);
const helpSection = ref<string | undefined>();
function showHelp(section?: string) { helpSection.value = section; helpOpen.value = true; }
/** First launch: set up the AI tools and the Claude account; dismissed once, remembered in localStorage. */
const welcomeOpen = ref(!welcomeSeen());
const ffmpegMissing = ref(false);

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
/** Three splitters: library width (edge next to the video), the loops drawer's height, and the right (Inspector / AI) column width. */
type SplitKind = "width" | "loops" | "inspector";
let splitStart: { kind: SplitKind; at: number; v: number } | null = null;
function startSplit(kind: SplitKind, e: PointerEvent) {
  const v = kind === "width" ? store.libraryWidth : kind === "loops" ? store.loopsHeight : store.inspectorWidth;
  splitStart = { kind, at: kind === "loops" ? e.clientY : e.clientX, v };
  (e.target as HTMLElement).setPointerCapture(e.pointerId);
}
function onSplit(e: PointerEvent) {
  const s = splitStart; if (!s) return;
  if (s.kind === "width") store.libraryWidth = Math.round(Math.min(Math.max(200, s.v + (e.clientX - s.at)), window.innerWidth * 0.5));
  else if (s.kind === "inspector") store.inspectorWidth = Math.round(Math.min(Math.max(240, s.v - (e.clientX - s.at)), window.innerWidth * 0.5));
  else store.loopsHeight = Math.round(Math.min(Math.max(60, s.v + (e.clientY - s.at)), window.innerHeight * 0.6));
}
function endSplit() { splitStart = null; }
/** The folder of the last saved / opened project, so the dialogs start where the user works. */
const lastFolder = () => { const p = store.lastProjectPath; return p && p.includes("/") ? p.slice(0, p.lastIndexOf("/")) : undefined; };
async function openProject() {
  if (store.dirty && !(await ask("Discard unsaved changes?", { title: "ForgeVideo", kind: "warning" }))) return;
  const p = await open({ multiple: false, defaultPath: lastFolder(), filters: [{ name: "ForgeVideo project", extensions: ["forgevideo", "json"] }] });
  if (typeof p === "string") await store.open(p);
}
async function saveProject(as = false) {
  let path: string | undefined;
  if (as || !(await api.projectSave().catch(() => null))) {
    const dir = lastFolder();
    const p = await save({ defaultPath: `${dir ? dir + "/" : ""}${store.project?.name ?? "Untitled"}.forgevideo`, filters: [{ name: "ForgeVideo project", extensions: ["forgevideo"] }] });
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
  if (e.key === "Escape") { if (helpOpen.value) helpOpen.value = false; else if (!store.exportOpen) store.setRange(null); return; }
  if (e.key === "?" && !meta) { e.preventDefault(); if (helpOpen.value) helpOpen.value = false; else showHelp(SHORTCUTS_SECTION); return; }
  if (helpOpen.value || welcomeOpen.value) return;
  if (e.code === "Space") { e.preventDefault(); if (store.clips.length) store.togglePlay(); }
  else if (meta && (e.key === "z" || e.key === "Z")) { e.preventDefault(); void (e.shiftKey ? store.redo() : store.undo()); }
  else if (meta && e.key === "t") { e.preventDefault(); void store.splitAtPlayhead(); }
  else if (meta && e.key === "j") { e.preventDefault(); void store.mergeSelected(); }
  else if ((e.key === "Backspace" || e.key === "Delete") && store.selected) { e.preventDefault(); void store.deleteSelected(); }
  else if (e.key === "ArrowLeft") { store.playing = false; store.seek(store.playhead - (e.shiftKey ? 1000 : 33)); }
  else if (e.key === "ArrowRight") { store.playing = false; store.seek(store.playhead + (e.shiftKey ? 1000 : 33)); }
  else if (e.key === "Home") { store.playing = false; store.seek(0); }
  else if (e.key === "End") { store.playing = false; store.seek(store.duration); }
  else if (e.key === "l" && !meta) { if (store.range) store.loopOn = !store.loopOn; }
  else if (meta && e.key === "s") { e.preventDefault(); void saveProject(e.shiftKey); }
  else if (meta && e.key === "o") { e.preventDefault(); void openProject(); }
  else if (meta && e.key === "i") { e.preventDefault(); void importDialog(); }
  else if (meta && e.key === "e") { e.preventDefault(); store.exportOpen = true; }
}

let unlistenDrop: (() => void) | undefined;
onMounted(async () => {
  await store.load();
  const st = await api.ffmpegStatus();
  ffmpegMissing.value = !st.ffmpeg || !st.ffprobe;
  window.addEventListener("keydown", onKey);
  unlistenDrop = await getCurrentWebview().onDragDropEvent((e) => {
    if (e.payload.type !== "drop") return;
    const paths = e.payload.paths.filter(isMediaPath);
    const skipped = e.payload.paths.filter((p) => !isMediaPath(p));
    if (skipped.length) {
      const names = skipped.slice(0, 2).map(basename).join(", ") + (skipped.length > 2 ? ` +${skipped.length - 2} more` : "");
      store.notify(`Skipped ${names} — not a supported media file`);
    }
    if (!paths.length) return;
    // Finder drops only ever feed the pool; say so when they land on the timeline.
    const pos = e.payload.position;
    const dpr = window.devicePixelRatio || 1;
    if (pos && document.elementFromPoint(pos.x / dpr, pos.y / dpr)?.closest("[data-testid=timeline-tracks]")) {
      store.notify("Added to the media pool — drag from the pool onto the timeline");
    }
    void store.poolAdd(paths);
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
      <span class="w-px h-5 bg-line mx-1" />
      <button class="px-2 py-1 rounded hover:bg-panel-2" title="Guide to every part of the app" data-testid="help-menu" @click="showHelp()">Help</button>
      <span class="flex-1" />
      <span v-if="store.error" class="text-danger truncate max-w-md mr-3" :title="store.error">{{ store.error }}</span>
      <span v-if="ffmpegMissing" class="text-danger mr-3">ffmpeg not found — brew install ffmpeg</span>
      <button class="px-2 py-1 rounded hover:bg-panel-2" :disabled="!store.clips.length" @click="store.togglePlay()">{{ store.playing ? '⏸ Pause' : '▶ Play' }}</button>
      <button
        class="ml-1 px-2 py-1 rounded border" :class="store.aiOpen ? 'border-accent text-accent bg-accent/10' : 'border-line hover:bg-panel-2'"
        title="Captions and highlights for shorts" :aria-pressed="store.aiOpen" data-testid="ai-toggle" @click="store.aiOpen = !store.aiOpen"
      >✦ AI</button>
      <button class="ml-1 px-3 py-1.5 rounded bg-accent text-black font-medium disabled:opacity-40" :disabled="!store.clips.length" @click="store.exportOpen = true">Export…</button>
      <button class="ml-1 w-7 h-7 rounded-full border border-line hover:bg-panel-2 font-semibold" title="Keyboard shortcuts (?)" aria-label="Help: keyboard shortcuts" @click="showHelp(SHORTCUTS_SECTION)">?</button>
    </header>

    <div class="flex-1 min-h-0 flex">
      <!-- library column: pinned loops above the media pool, left of the video -->
      <aside class="shrink-0 flex flex-col min-h-0 bg-panel" data-testid="library" :style="{ width: store.libraryWidth + 'px' }">
        <LoopsDrawer class="shrink-0" :style="{ height: store.loopsOpen ? store.loopsHeight + 'px' : 'auto' }" />
        <div
          v-if="store.loopsOpen" class="h-1 shrink-0 cursor-row-resize bg-line hover:bg-accent/60" title="Drag to resize Pinned loops" data-testid="loops-resize"
          @pointerdown="startSplit('loops', $event)" @pointermove="onSplit" @pointerup="endSplit" @pointercancel="endSplit"
        />
        <MediaPool class="flex-1 min-h-0" />
      </aside>
      <div
        class="w-1 shrink-0 cursor-col-resize bg-line hover:bg-accent/60" title="Drag to resize the library" data-testid="library-resize"
        @pointerdown="startSplit('width', $event)" @pointermove="onSplit" @pointerup="endSplit" @pointercancel="endSplit"
      />
      <Preview />
      <div
        class="w-1 shrink-0 cursor-col-resize bg-line hover:bg-accent/60" title="Drag to resize the side panel" data-testid="inspector-resize"
        @pointerdown="startSplit('inspector', $event)" @pointermove="onSplit" @pointerup="endSplit" @pointercancel="endSplit"
      />
      <AiPanel v-if="store.aiOpen" :style="{ width: store.inspectorWidth + 'px' }" />
      <Inspector v-else :style="{ width: store.inspectorWidth + 'px' }" />
    </div>

    <div
      class="h-1 shrink-0 cursor-row-resize bg-line hover:bg-accent/60" title="Drag to resize the timeline" data-testid="timeline-resize"
      @pointerdown="startResize" @pointermove="onResize" @pointerup="endResize" @pointercancel="endResize"
    />
    <div class="shrink-0" :style="{ height: store.timelineHeight + 'px' }">
      <Timeline />
    </div>

    <ExportDialog :open="store.exportOpen" @close="store.exportOpen = false" />
    <HelpDialog :open="helpOpen" :section="helpSection" @close="helpOpen = false" />
    <WelcomeDialog :open="welcomeOpen" @close="welcomeOpen = false" />
    <Banner />
  </div>
</template>
