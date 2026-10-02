<script lang="ts">
// Single source of truth for the hot-key list (also asserted by App.test.ts and mirrored in README.md).
export const SHORTCUTS: { keys: string; action: string }[] = [
  { keys: "Space", action: "Play / pause (a loaded loop restarts from its beginning)" },
  { keys: "⌘ Z / ⇧ ⌘ Z", action: "Undo / redo the last edit" },
  { keys: "⌘ T", action: "Split selected clip at playhead" },
  { keys: "⌘ J", action: "Join (merge) the selected clips (⇧-click to select several)" },
  { keys: "⌫ / Delete", action: "Delete selected clip" },
  { keys: "← / →", action: "Nudge playhead one frame" },
  { keys: "⇧ ← / ⇧ →", action: "Nudge playhead one second" },
  { keys: "Home / End", action: "Jump to start / end" },
  { keys: "L", action: "Loop the selected range" },
  { keys: "⌘ I", action: "Import media…" },
  { keys: "⌘ O", action: "Open project…" },
  { keys: "⌘ S", action: "Save project" },
  { keys: "⇧ ⌘ S", action: "Save project as…" },
  { keys: "⌘ E", action: "Export…" },
  { keys: "?", action: "Show this help" },
  { keys: "Esc", action: "Close dialog / clear the range" },
];

export const MOUSE: { keys: string; action: string }[] = [
  { keys: "Drag clip edge", action: "Trim in / out point (frame-snapped)" },
  { keys: "Drag V1 clip", action: "Reorder clips" },
  { keys: "Drag overlay / audio clip", action: "Move it in time, or to another layer / track" },
  { keys: "Drag from media pool", action: "Place on a track at that time" },
  { keys: "Double-click pool item", action: "Place it at the playhead" },
  { keys: "Click ruler", action: "Scrub the playhead (outside a range clears it)" },
  { keys: "Drag on ruler", action: "Select a range (yellow) · ⟳ loops it, Pin keeps it" },
  { keys: "Drag range edge", action: "Resize the range" },
  { keys: "Drag preview", action: "Reposition the crop, or place the selected overlay" },
  { keys: "Scroll on preview", action: "Zoom in or out (down to 0.25×), or resize the selected overlay" },
  { keys: "Drop media files", action: "Add to the media pool" },
];

/** One section of the in-app guide (Help in the top bar). `shortcuts` marks the section that renders the tables above. */
export interface GuideSection { id: string; title: string; intro: string; items: { label: string; text: string }[]; shortcuts?: boolean }

export const SHORTCUTS_SECTION = "shortcuts";

export const GUIDE: GuideSection[] = [
  {
    id: "overview", title: "Overview", intro: "ForgeVideo is a small editor for cutting one video for YouTube, Shorts, Square or LinkedIn. Editing never touches your files: the timeline is a list of instructions and only Export renders a new file with ffmpeg.",
    items: [
      { label: "Top bar", text: "New, Open and Save manage the project file; Import… adds media; ▶ Play previews; ✦ AI opens captions and highlights; Export… renders; Help opens this guide and ? the shortcuts." },
      { label: "Library column", text: "Left of the video: Pinned loops on top, the Media pool underneath. Drag the splitter between them, or the vertical bar on the right edge, to resize." },
      { label: "Preview", text: "The video with the chosen aspect frame. Drag to reposition the crop or a selected overlay or title; scroll to zoom." },
      { label: "Inspector", text: "Right of the preview: settings for whatever is selected (project, clip, overlay, title, audio, captions). With ✦ AI on, the AI panel takes its place. Drag its left edge to make it wider or narrower." },
      { label: "Timeline", text: "Bottom: text rows on top, overlay rows, the V1 video track, the captions row and audio tracks. Drag the bar above it to change its height." },
    ],
  },
  {
    id: "project", title: "Project", intro: "A project is a pretty-printed .forgevideo JSON file. Media stays where it is; paths inside the project folder are stored relative so the folder can move.",
    items: [
      { label: "New / Open / Save", text: "⌘ O opens, ⌘ S saves, ⇧ ⌘ S saves under a new name. A • after Save means there are unsaved changes; New and Open ask before discarding them." },
      { label: "Last project", text: "The last saved or opened file reopens on the next launch when it still loads. New forgets it, and the Open / Save dialogs start in its folder." },
      { label: "Aspect preset", text: "Select the project (click empty timeline space) to pick YouTube 16:9, Shorts/Reels 9:16, Square 1:1 or LinkedIn 4:5. Everything is rendered inside that frame." },
      { label: "Undo", text: "⌘ Z (or Ctrl Z) undoes the last edit, ⇧ ⌘ Z redoes it, up to 100 steps. The history is kept for the session and starts fresh when you open or create a project." },
      { label: "Old files", text: "Files saved by earlier versions open fine: missing fields get defaults and the timeline is tidied on load." },
    ],
  },
  {
    id: "pool", title: "Media pool", intro: "Every imported file lives in the pool, whether or not it is on the timeline.",
    items: [
      { label: "Adding media", text: "Import… or ⌘ I, or drop files from Finder anywhere in the window. Video also lands at the end of V1. The same file is never pooled twice." },
      { label: "Filter and view", text: "The dropdown filters All / Clips / Audio / Images with counts; switch between grid and list. Both choices are remembered." },
      { label: "Placing", text: "Drag an item onto a track to place it at that time, or double-click to place it at the playhead. Audio only goes on audio tracks; video and images only on video rows (a banner tells you why a drop was refused)." },
      { label: "Supported files", text: "Video: mp4, mov, m4v, mkv, webm, avi, mts, m2ts, 3gp, ts, mpg, mpeg, wmv, flv, mxf. Images: png, jpeg, webp, bmp, tiff. Audio: mp3, m4a, aac, wav, aiff, flac, ogg, opus, caf, m4b, wma. Formats the preview cannot play (mkv, avi, mts…) still export." },
    ],
  },
  {
    id: "v1", title: "V1 video track", intro: "The main track. Clips sit back to back with no gaps, so every edit ripples what follows.",
    items: [
      { label: "Trim", text: "Drag either edge of a clip. Trims snap to frames and a clip is never shorter than 100 ms." },
      { label: "Split", text: "Select a clip, put the playhead inside it and press ⌘ T. Works on overlay and audio clips too." },
      { label: "Join", text: "⇧-click two or more neighbouring pieces of the same file, then ⌘ J, right-click → Merge, or the Join button in the Inspector. Pieces must be continuous in the source." },
      { label: "Reorder / delete", text: "Drag a clip past its neighbour's midpoint to swap them. ⌫ deletes the selection." },
      { label: "Still images", text: "An image is a 5 s clip; trim it to any length. It is silent and previewed with a clock instead of a video." },
      { label: "Fades and transitions", text: "Fade in / out up to 5 s per clip. Between two clips choose Cut, Cross dissolve or Dip to black (100 – 3000 ms)." },
      { label: "Sound", text: "Volume 0 – 200 % and mute per clip, plus the track mute (speaker icon in the gutter) and track fader under the V1 label. Right-click → Split audio from video moves a clip's sound to an audio track and mutes the video." },
      { label: "Rename", text: "Right-click any clip on any track → Rename… (blank restores the file name)." },
    ],
  },
  {
    id: "overlays", title: "Overlay rows", intro: "V2, V3, … composite on top of V1, higher rows on top. Use them for picture-in-picture, logos and badges.",
    items: [
      { label: "Rows", text: "+ Track → Video adds a row; ✕ in the gutter removes it with its clips." },
      { label: "Placing", text: "Clips are free-positioned: drag in time or to another row. A clip dropped onto another pushes it aside; rows never overlap." },
      { label: "Framing", text: "Drag the selected overlay in the preview to place it, scroll to resize. Stills start as a 35 % badge bottom-right, video full frame." },
      { label: "Look and sound", text: "Opacity 0 – 1 and opacity fades per clip. A video overlay's soundtrack is heard: clip volume + mute, and a row mute + fader in the gutter." },
    ],
  },
  {
    id: "text", title: "Text rows (titles)", intro: "T1, T2, … at the top of the timeline hold titles. They are drawn by the app and composited over everything on export.",
    items: [
      { label: "Rows and titles", text: "+ Track → Text adds a row; each row's + Text adds a 5 s “Title” at the playhead. Drag to move (also between rows), drag the edges for length, ⌘ T splits, ⌫ deletes." },
      { label: "Style", text: "The Inspector's Title section edits the text (Enter = new line, up to 500 characters), any installed font, size as a percentage of the frame height, colour, fades, and an optional rounded backdrop with its own colour and opacity." },
      { label: "Position", text: "Drag the selected title in the preview to place it, scroll to resize." },
      { label: "Limits", text: "No automatic line wrapping; titles after the end of V1 are ignored; audio-only exports skip them." },
    ],
  },
  {
    id: "audio", title: "Audio tracks", intro: "Any number of labelled tracks (Music, SFX, Narration, Other) mixed under the video.",
    items: [
      { label: "Tracks", text: "+ Track → Audio / Music / Narration / SFX adds a labelled track; the label is editable; ✕ removes it with its clips." },
      { label: "Clips", text: "Drop audio files or the sound of a video. Each clip has volume, fade in / out and mute. Clips move freely and push each other apart." },
      { label: "Track controls", text: "Every speaker icon is a mute: green = on, grey = muted. The 0 – 1 fader under the label scales every clip on that track." },
      { label: "Export", text: "All tracks are mixed with V1 and heard overlays, then trimmed to the video length." },
    ],
  },
  {
    id: "loops", title: "Range selection & pinned loops", intro: "Like GarageBand's cycle region: select a span of the timeline, loop it while you work, and keep the spans you care about.",
    items: [
      { label: "Select a range", text: "Drag on the ruler. The yellow band covers every row; its edges snap to clip boundaries and the playhead. Drag either handle on the ruler to extend it. Click outside the band or press Esc to clear it." },
      { label: "Loop it", text: "⟳ Loop in the timeline toolbar or L. Playback wraps at the end of the range, and Space with a loaded loop restarts it from the beginning." },
      { label: "Pin it", text: "Pin keeps the range in the Pinned loops drawer and in the project file. The same span cannot be pinned twice: you get a notice and the existing loop is selected." },
      { label: "Drawer", text: "▶ plays a loop on repeat, clicking the name shows it on the timeline (and scrolls to it), double-click renames it, Export renders just that span, ✕ unpins." },
      { label: "Fixed in time", text: "Loops keep their timeline position when you edit; shortening V1 clamps or drops them." },
    ],
  },
  {
    id: "preview", title: "Preview & framing", intro: "The preview shows exactly what Export will render: the aspect frame, crop, fades, dissolves, overlays, titles and the current caption.",
    items: [
      { label: "Transport", text: "Space plays and pauses, ← / → step a frame (⇧ = one second), Home / End jump. Playing starts at the loop start when a loop is on and the playhead is outside it." },
      { label: "Reposition / zoom", text: "Drag the picture to move it inside the frame; scroll or use the Inspector's Zoom slider (−100 … 0 … +100). Zoomed in, the frame is a crop of the source. Zoomed out, the source shrinks onto black and can sit partly outside the frame." },
      { label: "Phone footage", text: "Rotated clips use their display orientation automatically." },
      { label: "Unplayable media", text: "Formats the built-in player cannot decode show a banner instead of freezing; they still export." },
    ],
  },
  {
    id: "ai", title: "AI mode", intro: "✦ AI replaces the Inspector with a panel for captions and highlight picks. It runs on tools installed on this Mac: whisper for speech and Claude Code for picks. No API key is needed.",
    items: [
      { label: "Setup", text: "Install AI tools… opens Terminal and installs whisper.cpp, its model and Claude Code. Sign in to Claude opens the login in Terminal; the app never stores the account itself." },
      { label: "Captions", text: "Transcribe listens to every V1 source with speech. Cues follow their source clip, so trims and moves never desync them. The caption row appears under V1 and the current cue shows in the preview." },
      { label: "Editing captions", text: "Double-click a caption on the timeline, or select the CC row and click a line in the Inspector's Captions list. Enter or clicking away saves, Esc cancels, blank removes the cue. The on/off switch hides captions and skips the .srt." },
      { label: "Highlights", text: "Find highlights sends the transcript to Claude and marks up to ten 10 – 180 s picks as green bands (hidden while AI is off). Each card has ▶ to loop it, Pin loop, Show, and Export short, which exports just that span." },
      { label: "Transcript", text: "Copy puts the transcript on the clipboard, one line per cue. Highlights are not shifted by edits, so find them again after restructuring." },
    ],
  },
  {
    id: "export", title: "Export", intro: "⌘ E or Export… opens the dialog. The box explains what will happen before anything is rendered.",
    items: [
      { label: "Fast trim", text: "A single untouched h264/hevc clip that already matches the preset is copied without re-encoding; cuts snap to the nearest keyframe." },
      { label: "Render", text: "Everything else is encoded with the Mac's hardware H.264 encoder (Draft / Standard / High quality) with AAC audio. The dialog lists why a re-encode was needed." },
      { label: "Audio only", text: "Writes an .m4a for podcasts; titles and overlays' pictures are skipped, their sound is kept." },
      { label: "Range", text: "With a range or loop selected the dialog preselects Range · name; switch to Whole timeline to render everything. The file name gets the loop name as a suffix." },
      { label: "Captions", text: "When captions are on, a .srt with the same name is written beside the video. Captions are never burned in." },
      { label: "Progress", text: "Progress bar and elapsed time; Cancel stops ffmpeg and removes the partial file; Reveal in Finder shows the result." },
    ],
  },
  { id: SHORTCUTS_SECTION, title: "Keyboard & mouse", intro: "Every shortcut. Keys are ignored while typing in a field.", items: [], shortcuts: true },
];
</script>

<script setup lang="ts">
import { ref, watch } from "vue";

const props = defineProps<{ open: boolean; section?: string }>();
const emit = defineEmits<{ close: [] }>();
const current = ref(GUIDE[0]!.id);
const section = () => GUIDE.find((g) => g.id === current.value) ?? GUIDE[0]!;
const body = ref<HTMLElement | null>(null);
function go(id: string) { current.value = id; body.value?.scrollTo({ top: 0 }); }
watch(() => props.open, (o) => { if (o) current.value = GUIDE.some((g) => g.id === props.section) ? props.section! : GUIDE[0]!.id; }, { immediate: true });
</script>

<template>
  <div v-if="open" class="fixed inset-0 z-50 bg-black/60 flex items-center justify-center" @pointerdown.self="emit('close')">
    <div class="w-[820px] max-w-[95vw] h-[600px] max-h-[85vh] rounded-lg bg-panel border border-line shadow-2xl flex flex-col text-xs" role="dialog" aria-label="Help" data-testid="help">
      <div class="flex items-center justify-between px-4 h-11 border-b border-line shrink-0">
        <h2 class="text-sm font-semibold">Help</h2>
        <button class="text-muted hover:text-fg" aria-label="Close" @click="emit('close')">✕</button>
      </div>
      <div class="flex-1 min-h-0 flex">
        <nav class="w-48 shrink-0 border-r border-line overflow-y-auto py-2" aria-label="Help sections">
          <button
            v-for="g in GUIDE" :key="g.id" class="block w-full text-left px-4 py-1.5 hover:bg-panel-2" :class="g.id === current ? 'text-accent bg-accent/10' : 'text-fg'"
            :aria-current="g.id === current ? 'true' : undefined" data-testid="help-nav" @click="go(g.id)"
          >{{ g.title }}</button>
        </nav>
        <div ref="body" class="flex-1 min-w-0 overflow-y-auto px-5 py-4" data-testid="help-body">
          <h3 class="text-base font-semibold mb-1">{{ section().title }}</h3>
          <p class="text-muted mb-4 leading-relaxed">{{ section().intro }}</p>
          <dl v-if="!section().shortcuts" class="space-y-3">
            <div v-for="i in section().items" :key="i.label">
              <dt class="font-semibold text-fg">{{ i.label }}</dt>
              <dd class="text-fg/90 leading-relaxed">{{ i.text }}</dd>
            </div>
          </dl>
          <template v-else>
            <table class="w-full">
              <tbody>
                <tr v-for="s in SHORTCUTS" :key="s.keys" class="border-b border-line/50 last:border-0">
                  <td class="py-1 pr-3 w-32"><kbd class="font-mono rounded border border-line bg-panel-2 px-1.5 py-0.5">{{ s.keys }}</kbd></td>
                  <td class="py-1 text-fg">{{ s.action }}</td>
                </tr>
              </tbody>
            </table>
            <h4 class="uppercase tracking-wide text-[10px] text-muted mt-4 mb-1">Mouse</h4>
            <table class="w-full">
              <tbody>
                <tr v-for="s in MOUSE" :key="s.keys" class="border-b border-line/50 last:border-0">
                  <td class="py-1 pr-3 w-40 text-muted">{{ s.keys }}</td>
                  <td class="py-1 text-fg">{{ s.action }}</td>
                </tr>
              </tbody>
            </table>
          </template>
        </div>
      </div>
      <div class="flex justify-end px-4 h-11 items-center border-t border-line shrink-0">
        <button class="rounded bg-accent text-black font-medium px-3 py-1.5" @click="emit('close')">Close</button>
      </div>
    </div>
  </div>
</template>
