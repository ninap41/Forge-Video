# ForgeVideo

A deliberately tiny Mac video editor: a ripple-edited main track, overlay layers for logos and
composite shots, labelled audio tracks, a media pool, trim / split / fade / cross dissolve, aspect
presets (YouTube 16:9, Shorts 9:16, Square, LinkedIn 4:5) with reposition, and fast H.264 export
through Apple VideoToolbox. Editing never touches media; the timeline is metadata.

Stack: Vue 3 + TypeScript + Pinia + Tailwind → Tauri 2 → Rust → `ffmpeg`/`ffprobe` subprocesses.

## Features

**Media pool** — the section under the timeline. Every imported file lives here, in Clips / Audio /
Images tabs, as thumbnails or a list (your choice is remembered). Add files with **Import…**, **⌘I**,
the per-tab Import button, or by dropping them anywhere in the window. Drag an item onto a track to
place it at that time, or double-click to place it at the playhead. Accepted: mp4, mov, m4v, mkv,
webm, avi, mts, m2ts · mp3, m4a, aac, wav, aiff, flac · png, jpg, jpeg, webp.

**V1 · video** — the main track. Always contiguous: trimming or deleting ripples everything after.
Video *or still images* (a still defaults to 5 s and stretches as far as you drag it). Per clip:
frame-snapped trim handles, drag to reorder, fade in/out, volume and mute, and Cut / Cross dissolve /
Dip to black into the next clip. The speaker icon in the gutter mutes the whole track.

**V2, V3, … · overlay layers** — composited above V1 for logos, lower-thirds, B-roll and
picture-in-picture. Clips are free-positioned and silent; each layer keeps its clips from overlapping
(a clip you drop pushes what it lands on). Select an overlay and drag it in the preview to place it,
scroll to resize; PNGs start as a small bottom-right badge, video starts full-frame. Fade in/out per
clip. **+ Layer** in the gutter adds a row; ✕ removes one.

**Audio tracks** — as many as you like, each with a label (Music, SFX, Narration, Other, or your own
text; click it to rename) and a track mute. Clips are free-positioned, can be dragged between tracks,
and have volume, fade in/out and mute. Everything is mixed under the video audio and trimmed to the
video length on export.

**Split** the selected clip at the playhead with **⌘T**, on any track. The left half keeps its id;
fades and transitions move to the outer ends.

**Preview** mirrors the export: crop math, fades, dissolve opacity, every overlay layer, and every
audio clip under the playhead. Drag the timeline's top edge to resize it.

**Export (⌘E)** stream-copies when a single untouched clip already matches the preset, otherwise
re-encodes with `h264_videotoolbox` (Draft / Standard / High) and tells you why. Audio-only `.m4a`
for podcasts. Progress, cancel, Reveal in Finder.

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
  · `MuteToggle.vue` (green speaker = audio on).
- `src-tauri/src/project` model + JSON persistence (v1 → v2 migration) · `timeline` pure edit ops
  (`relayout` keeps V1 contiguous and free lanes non-overlapping) · `media` ffprobe (stills get a
  default length) ·
  `cache` thumbnails/waveforms in `~/Library/Caches/ForgeVideo` · `render` export planner + ffmpeg
  filter graph · `jobs` background jobs + progress events · `capture` reserved for recording (Phase 3).
- `src-tauri/vendor/wry/` is a local copy of wry 0.55.1 with one fix in `src/wkwebview/drag_drop.rs`:
  upstream panics when a drag advertises file names but carries none (file promises). Wired in via
  `[patch.crates-io]` in `src-tauri/Cargo.toml`; drop it once upstream guards that unwrap.

## Keys

Press **?** or click the **?** button at the right of the toolbar to see this list inside the app.

| Keys | Action |
|---|---|
| Space | Play / pause |
| ⌘ T | Split selected clip at playhead |
| ⌫ / Delete | Delete selected clip |
| ← / → | Nudge playhead one frame |
| ⇧ ← / ⇧ → | Nudge playhead one second |
| Home / End | Jump to start / end |
| ⌘ I | Import media… |
| ⌘ O | Open project… |
| ⌘ S | Save project |
| ⇧ ⌘ S | Save project as… |
| ⌘ E | Export… |
| ? | Show shortcuts |
| Esc | Close dialog |

The table is generated from `SHORTCUTS` in `src/components/HelpDialog.vue`; keep them in sync.

### Mouse

| Gesture | Action |
|---|---|
| Drag clip edge | Trim in / out point (frame-snapped) |
| Drag V1 clip | Reorder clips |
| Drag overlay / audio clip | Move it in time, or to another layer / track |
| Drag from media pool | Place on a track at that time |
| Double-click pool item | Place it at the playhead |
| Click ruler / drag | Scrub the playhead |
| Drag preview | Reposition the crop, or place the selected overlay |
| Scroll on preview | Zoom the crop, or resize the selected overlay |
| Drop media files | Add to the media pool |
| Drag timeline top edge | Resize the timeline panel |
