# ForgeVideo

A deliberately tiny Mac video editor: trim, split, reorder, ripple, fade, cross dissolve, one music
track, aspect presets (YouTube 16:9, Shorts 9:16, Square, LinkedIn 4:5) with reposition, and fast
H.264 export through Apple VideoToolbox. Editing never touches media; the timeline is metadata.

Stack: Vue 3 + TypeScript + Pinia + Tailwind → Tauri 2 → Rust → `ffmpeg`/`ffprobe` subprocesses.

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
- `src-tauri/src/project` model + JSON persistence · `timeline` pure edit ops · `media` ffprobe ·
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
