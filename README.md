# ForgeVideo

A deliberately tiny Mac video editor: a ripple-edited main track, overlay layers for logos and
composite shots, labelled audio tracks, a media pool, trim / split / fade / cross dissolve, aspect
presets (YouTube 16:9, Shorts 9:16, Square, LinkedIn 4:5) with reposition, and fast H.264 export
through Apple VideoToolbox. Editing never touches media; the timeline is metadata.

Stack: Vue 3 + TypeScript + Pinia + Tailwind → Tauri 2 → Rust → `ffmpeg`/`ffprobe` subprocesses.

## Features

**Media pool** — the column to the left of the video, under Pinned loops. Every imported file lives here, in All / Clips /
Audio / Images tabs, as thumbnails or a list (your choice is remembered). Add files with **Import…**,
**⌘I**, the per-tab Import button, or by dropping them anywhere in the window; the same file is never
pooled twice. Drag an item onto a track to place it at that time, or double-click to place it at the
playhead. A small banner explains any drop that cannot land. Accepted: mp4, mov, m4v, mkv, webm, avi,
mts, m2ts, 3gp, ts, mpg, mpeg, wmv, flv, mxf · mp3, m4a, aac, wav, aiff, aif, flac, ogg, oga, opus, caf,
m4b, wma · png, jpg, jpeg, webp, bmp, tif, tiff. Anything ffmpeg decodes exports fine; the preview can
only play what macOS WebKit plays (mp4/mov/m4v and common audio) and says so for the rest.

**V1 · video** — the main track. Always contiguous: trimming or deleting ripples everything after.
Video *or still images* (a still defaults to 5 s and stretches as far as you drag it). Per clip:
frame-snapped trim handles, drag to reorder, fade in/out, volume and mute, and Cut / Cross dissolve /
Dip to black into the next clip. The speaker icon in the gutter mutes the whole track and the slider under it is a track fader (0–100 %) on top of each clip's volume.

**V2, V3, … · overlay layers** — composited above V1 for logos, lower-thirds, B-roll and
picture-in-picture. Clips are free-positioned; each layer keeps its clips from overlapping
(a clip you drop pushes what it lands on). Select an overlay and drag it in the preview to place it,
scroll to resize; PNGs start as a small bottom-right badge, video starts full-frame. Fade in/out and
a constant **Opacity** per clip, so a half-transparent video over V1 makes a composite shot. A video overlay's own sound plays too: volume and mute per clip in the Inspector, plus a mute
and fader for the whole row in the gutter, all mixed into the export. Projects saved before this
load their overlays muted. **+ Track → Video track** in the gutter adds a row; ✕ removes one.

**Text rows T1, T2, …** — titles burned into the video, above every other layer (higher rows on top).
**+ Track → Text track** adds a row, ✕ removes it. Each row's **+ Text** drops a 5 s title at the
playhead; drag it in time or onto another row, trim its length from either edge, ⌘T splits it, ⌫ deletes it. The Inspector's **Title** section takes multi-line text (Enter for a new line), **any font
installed on the Mac**, a size slider, a colour well that opens the macOS colour panel, and an optional
**Backdrop**: a rounded rectangle behind the text in any colour and opacity. Drag the title in the
preview to place it, scroll to resize. Text is drawn by the app itself (not ffmpeg), so the export shows
exactly what the preview did. Captions from AI mode are separate and still go to an `.srt`.

**Range selection & pinned loops** — drag on the ruler to select a span (yellow); drag either handle to
resize it, click outside it or press Esc to clear it. **⟳ Loop** (or **L**) plays the span on repeat.
**Pin** keeps it in the **Pinned loops** drawer above the media pool, left of the video, saved with the project: play a loop,
click its name to show it, **Export** it on its own (the Export dialog preselects **Range**), or unpin it.
AI highlight cards have the same ▶ and **Pin loop** buttons next to Export short. Pinning or picking a
loop scrolls the timeline to it. The library column and the Pinned loops drawer both resize by dragging
their edges.

**Audio tracks** — as many as you like, each with a label (Music, SFX, Narration, Other, or your own
text; click it to rename), a track mute and a track fader. Clips are free-positioned, can be dragged between tracks,
and have volume, fade in/out and mute. Everything is mixed under the video audio and trimmed to the
video length on export.

**Split** the selected clip at the playhead with **⌘T**, on any track. The left half keeps its id;
fades and transitions move to the outer ends. **Merge** undoes it: ⇧-click the pieces (two or more
neighbours on the same track, from the same file, in order) and press **⌘J** (join) or right-click → **Merge N clips**.
The first piece keeps its id and name; fades and the transition come back from the outer ends.

**Preview** mirrors the export: crop math, fades, dissolve opacity, every overlay layer, and every
audio clip under the playhead. Drag the timeline's top edge to resize it.

**Export (⌘E)** stream-copies when a single untouched clip already matches the preset, otherwise
re-encodes with `h264_videotoolbox` (Draft / Standard / High) and tells you why. Audio-only `.m4a`
for podcasts. Progress, cancel, Reveal in Finder.

**AI mode (✦ AI)** turns a long recording into shorts, using tools on this Mac instead of an API
key. It needs the optional setup in step 7.

1. **Transcribe** turns the speech on V1 into captions with `whisper-cli`. A **Copy** button puts the whole transcript on the clipboard as plain text. Click the **CC** label to select the captions track and switch it off in the Inspector (no preview caption, no .srt) without losing the transcript. They appear as a caption
   row under V1 and over the preview, and follow every trim, split and reorder. Double-click a
   caption to correct it. Export writes a `.srt` beside the video; captions are not burned in.
2. **Find highlights** sends the transcript (text only) to the Claude Code CLI, which suggests up to
   ten sections that stand on their own. Each is marked on the timeline and listed with its edit
   plan: what to keep, what to cut, fades, and notes for things to do by hand.
3. **Export short** selects that section on the timeline and opens the Export dialog on it, so the
   rendered file covers just the highlight. The plan's keep / cut steps are there to follow by hand.

Only V1 is transcribed. The green highlight bands show while the AI panel is open and hide when it is
closed (the highlights are kept). Highlights are not moved by later edits, so find them again after
restructuring the timeline. Captions can be corrected in the Inspector's Captions list (select the CC
track) as well as by double-clicking them on the timeline.

**Projects** are pretty JSON `.forgevideo` files with media paths stored relative to the project
folder. Files saved before audio tracks existed load their music bed as one "Music" track.

## How rendering works

ForgeVideo does not link against FFmpeg or talk to VideoToolbox itself. Rust spawns the `ffmpeg` and
`ffprobe` command-line tools installed on the Mac and asks ffmpeg to encode with
`h264_videotoolbox`, Apple's hardware encoder. **The app will not import or export anything until
FFmpeg is installed.** It looks in `/opt/homebrew/bin`, then `/usr/local/bin`, then `PATH`.

## Setup (macOS)

### 1. Install Homebrew

Homebrew is the package manager that provides FFmpeg. Skip this if `brew --version` already works.

```sh
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
```

The installer prints two `echo … >> ~/.zprofile` / `eval "$(/opt/homebrew/bin/brew shellenv)"`
lines at the end. Run them, then open a new terminal and check:

```sh
brew --version
```

### 2. Install FFmpeg

```sh
brew install ffmpeg
```

Verify both tools are found and that the VideoToolbox encoder is present:

```sh
ffmpeg -version | head -1
ffprobe -version | head -1
ffmpeg -hide_banner -encoders | grep h264_videotoolbox     # must print one line
```

If the last command prints nothing, the build lacks hardware encoding; reinstall with
`brew reinstall ffmpeg`. To use an FFmpeg installed elsewhere, set
`FORGE_FFMPEG=/path/to/ffmpeg` and `FORGE_FFPROBE=/path/to/ffprobe` before launching.

That is everything an **end user** needs. The steps below are for building and developing the app.

### 3. Xcode command line tools

```sh
xcode-select --install
```

### 4. Rust

```sh
curl https://sh.rustup.rs -sSf | sh
source ~/.cargo/env          # once per non-login shell; rustup also adds it to ~/.zshrc
```

### 5. Node 20+

```sh
brew install node
node --version
```

### 6. JavaScript dependencies

From the repo root:

```sh
npm install
```

Rust crates are fetched automatically on the first `cargo` or `tauri` command.

### 7. AI mode (optional)

Everything else works without this step. The quickest route is the **Install AI tools…** button in
the AI panel (or `bash scripts/install-ai.sh`): it opens Terminal and installs Homebrew, ffmpeg,
whisper.cpp, the speech model and Claude Code, skipping whatever is already there. By hand:

```sh
brew install whisper.cpp
mkdir -p "$HOME/Library/Application Support/ForgeVideo/models"
curl -L -o "$HOME/Library/Application Support/ForgeVideo/models/ggml-base.en.bin" \
  https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin
```

`ggml-base.en.bin` (about 150 MB) is English only. To use another model, point
`FORGE_WHISPER_MODEL` at its file.

Install [Claude Code](https://claude.com/claude-code) (`brew install --cask claude-code` or the
installer on that page). The first time ForgeVideo opens, a welcome dialog offers both steps
(**Install AI tools…** and **Sign in to Claude**); **Skip for now** and it never asks again. Later,
in the AI panel, press **Sign in to Claude**: it opens Terminal on
`claude auth login`, which sends you to the browser, and the panel shows the signed-in email once
that finishes (**Sign out** next to it runs `claude auth logout`). ForgeVideo runs Claude as
`claude -p` with no tools; usage counts against that Claude subscription.

The AI panel shows which of the four (whisper-cli, model, Claude Code, account) is missing. `FORGE_WHISPER` and `FORGE_CLAUDE` override
where the binaries are looked up.

## Run

```sh
npm run tauri dev        # Vite dev server on :1420 + Rust app with hot reload
npm run tauri build      # release .app / .dmg under src-tauri/target/release/bundle
```

If `tauri dev` fails with "port 1420 is already in use", a previous Vite is still running:
`pkill -f node_modules/.bin/vite`.

## Tests

There are two suites. Both run fully offline; the Rust suite spawns the real `ffmpeg`/`ffprobe`
against the small fixtures in `tests/fixtures/`, so FFmpeg must be installed first (steps 1–2 above).

### Frontend (vitest, happy-dom, @vue/test-utils)

```sh
npm test                 # run once: utils, types, API wrapper, Pinia store, every component, App shell
npm run test:watch       # re-run on save
npm run test:coverage    # v8 coverage report (text summary + coverage/ directory)
npx vitest run src/components/Timeline.test.ts     # a single file
npx vitest run -t "split"                          # tests whose name matches
```

Tests live next to the code as `src/**/*.test.ts`. Rust is never called: `src/api/tauri.ts` is
mocked through `src/test/fixtures.ts` (`mockApi`, `resolveWith`, `project`, `clip`, `music`), and
browser APIs the components need (ResizeObserver, canvas, media elements) are stubbed in
`src/test/setup.ts`.

### Rust (cargo test)

```sh
cd src-tauri
source ~/.cargo/env      # if `cargo` is not on PATH
cargo test               # unit tests in every module + tests/export_e2e.rs
cargo test timeline      # only tests whose path contains "timeline"
cargo test -- --nocapture    # show println!/log output
```

What is covered: project model and JSON persistence, timeline ops, export planner, ffmpeg filter
graph, the ffmpeg runner (progress, failure, cancel), ffprobe parsing, thumbnail/waveform caches,
job registry, every Tauri command (driven through `tauri::test::mock_app`), and two real exports
(stream copy and a multi-clip render with dissolve, music and vertical crop).

The cache tests write under `~/Library/Caches/ForgeVideo/`; exports and probes use temp dirs.

### Type check + production bundle

```sh
npm run build            # vue-tsc --noEmit (includes the test files) + vite build
```

### Everything at once

```sh
npm test && npm run build && (cd src-tauri && cargo test)
```

## Layout

- `src/` Vue UI. `src/api/tauri.ts` is the only place that calls Rust.
- `src/components/`: `Timeline.vue` (rows, gutter, drags, pool drops) · `MediaPool.vue` ·
  `ClipBlock.vue` (V1) and `FreeBlock.vue` (overlay + audio clips) · `Preview.vue` · `Inspector.vue`
  · `AiPanel.vue` (captions, highlights, edit plans) · `MuteToggle.vue` (green speaker = audio on).
- `src-tauri/src/project` model + JSON persistence (v1 → v2 migration) · `timeline` pure edit ops
  (`relayout` keeps V1 contiguous and free lanes non-overlapping) · `media` ffprobe (stills get a
  default length) ·
  `cache` thumbnails/waveforms in `~/Library/Caches/ForgeVideo` · `render` export planner + ffmpeg
  filter graph · `jobs` background jobs + progress events · `ai` whisper transcription, captions,
  Claude Code highlights and the short builder · `capture` reserved for recording (Phase 3).
- `src-tauri/vendor/wry/` is a local copy of wry 0.57.0 with one fix in `src/wkwebview/drag_drop.rs`:
  upstream panics when a drag advertises file names but carries none (file promises). Wired in via
  `[patch.crates-io]` in `src-tauri/Cargo.toml`; drop it once upstream guards that unwrap.

## Keys

Press **?** or click the **?** button at the right of the toolbar to see this list inside the app.

| Keys | Action |
|---|---|
| Space | Play / pause (a loaded loop restarts from its beginning) |
| ⌘ T | Split selected clip at playhead |
| ⌘ J | Join (merge) the selected clips (⇧-click to select several) |
| ⌫ / Delete | Delete selected clip |
| ← / → | Nudge playhead one frame |
| ⇧ ← / ⇧ → | Nudge playhead one second |
| Home / End | Jump to start / end |
| L | Loop the selected range |
| ⌘ I | Import media… |
| ⌘ O | Open project… |
| ⌘ S | Save project |
| ⇧ ⌘ S | Save project as… |
| ⌘ E | Export… |
| ? | Show shortcuts |
| Esc | Close dialog / clear the range |

The table is generated from `SHORTCUTS` in `src/components/HelpDialog.vue`; keep them in sync.

### Mouse

| Gesture | Action |
|---|---|
| Drag clip edge | Trim in / out point (frame-snapped) |
| Drag V1 clip | Reorder clips |
| Right-click V1 clip | **Split audio from video**: sound moves to an audio track, the clip is muted |
| Right-click any clip | **Rename…**: name the clip on the timeline (blank restores the file name) |
| ⇧-click clips, right-click | **Merge N clips**: join neighbouring pieces of one file back into a single clip |
| Drag overlay / audio clip | Move it in time, or to another layer / track |
| Drag from media pool | Place on a track at that time |
| Double-click pool item | Place it at the playhead |
| Click ruler | Scrub the playhead (outside a range clears it) |
| Drag on ruler | Select a range (yellow) · ⟳ loops it, Pin keeps it |
| Drag range edge | Resize the range |
| Drag preview | Reposition the picture, or place the selected overlay / title |
| Scroll on preview | Zoom in or out (down to 0.25×), or resize the selected overlay / title |
| Drop media files | Add to the media pool |
| Drag timeline top edge | Resize the timeline panel |
