# ForgeVideo

A deliberately tiny macOS video editor for Matt Katyella. Vue 3 + TypeScript + Pinia + Tailwind in a
Tauri 2 shell; Rust backend drives `ffmpeg`/`ffprobe` as subprocesses. Editing never touches media:
the timeline is metadata (integer milliseconds) and only Export renders.

## Commands

Setup once, in order: install Homebrew → `brew install ffmpeg` (the app spawns the `ffmpeg`/`ffprobe` CLIs and
asks for `h264_videotoolbox`; nothing works without them) → `xcode-select --install` → Rust via rustup → Node 20+ →
`npm install`. Full steps and verification commands are in README.md.

```sh
npm run tauri dev                       # run the app
npm test                                # vitest: utils, types, API wrapper, store, every component, App shell
npm run test:watch                      # vitest in watch mode
npm run test:coverage                   # same, with v8 coverage
npm run build                           # vue-tsc type check (includes tests) + vite build
cd src-tauri && cargo test              # Rust unit tests + tests/export_e2e.rs (spawns real ffmpeg)
npm test && npm run build && (cd src-tauri && cargo test)   # everything
```

Non-login shells need `source ~/.cargo/env` before `cargo`. Run all three checks before reporting work done.

## Supported features (V1)

Media pool (bottom of the timeline panel)
- Every imported file lives in the pool (`Project.pool`) with tabs All / Clips / Audio / Images, grid or list view (persisted in localStorage). **Import…**, **⌘I** and OS drag-and-drop add to the pool; video also lands on V1. Pointer-drag (never HTML5 DnD) a pool item onto a track, or double-click to place it at the playhead. The same file (compared by canonical path) is never pooled twice, and `project_open` re-probes the pool so stale `MediaInfo` (e.g. an mp3 whose cover art once read as video) is corrected. `probe.rs` ignores cover-art streams and treats MJPEG with a duration as video. `Banner.vue` shows `store.notify(...)` for a few seconds: duplicate imports, audio dropped on a video row, video/images dropped on an audio row, audio dropped with no audio track, and Finder drops that land on the timeline (they still go to the pool).

V1 video track (`Project.clips`)
- Any video ffmpeg decodes (extension list in `src/utils/media.ts`: mp4, mov, m4v, mkv, webm, avi, mts, m2ts, 3gp, ts, mpg, mpeg, wmv, flv, mxf) **or a still image** (png/jpeg/webp/bmp/tiff; 5 s by default, trim to any length, rendered via `-loop 1 -t`, silent, previewed with a wall-clock instead of a `<video>`). Contiguous, ripple-edited.
- **Trim** in/out points by dragging clip edges (frame-snapped, min clip 100 ms), rippling everything after.
- **Split** the selected clip at the playhead (**⌘T**); left half keeps its id, fades/transition move to the right ends. Works for V2 and audio clips too.
- **Delete** (**⌫**) and **reorder** by dragging a clip past its neighbour's midpoint. The timeline is always contiguous.
- **Fade in / fade out** per clip (video + audio), capped at 5 s or the clip length.
- **Transitions** between clips: Cut, **Cross dissolve**, **Dip to black** (100–3000 ms, clamped to half the shorter neighbour; the last clip never has one).
- **Split audio from video** (right-click a V1 clip): `timeline::detach_audio` copies the clip's range, volume and fades to an `AudioClip` on the first audio track (created if none) and mutes the video clip.
- **Rename** (right-click any clip on any track → Rename…): `name: Option<String>` on `Clip` / `OverlayClip` / `AudioClip`, set by the one `clip_rename` command (`timeline::rename`, trimmed, ≤ 80 chars, blank = file name). Splits and detached audio keep the name; display goes through `clipName()` in `src/types/project.ts`.
- **Volume** per clip (0–200 %) and **mute**, plus a track-level mute (`Project.video_muted`, speaker icon in the V1 gutter) that silences every V1 clip.

Overlay layers V2, V3, … (`Project.overlays`, `Project.overlay_layers`)
- Any number of overlay rows for composite shots; each `OverlayClip.layer` picks its row and higher layers composite on top. Free-positioned, silent clips: video or stills (default 5 s, stretch by trimming). No overlap within a layer (a placed clip pushes what it lands on), no transitions. The gutter's single **+ Track** button opens a menu (Video / Audio / Music / Narration / SFX): Video adds an overlay row (labelled V2, V3, … · video), the rest add an audio track with that label; ✕ removes a row and its clips. Per-clip opacity fades and a `Placement` (width as a fraction of the frame + centre); stills default to a 35 % bottom-right badge, video to full frame. Drag / scroll-wheel the selected overlay in the preview. Overlays that start after V1 ends are ignored on export.

Audio tracks (`Project.audio_tracks`)
- Any number of labelled tracks (Music / SFX / Narration / Other, free text) with free-positioned clips (mp3/m4a/aac/wav/aiff/aif/flac/ogg/oga/opus/caf/m4b/wma or the audio of a video): volume, fade in/out, mute per clip, mute per track. Every mute control is the `MuteToggle` speaker icon: green = audio on, grey struck-through = muted. Mixed under V1 audio with `amix` and trimmed to the video length on export. v1 project files with a `music` bed load as one "Music" track.

Output
- Aspect presets: **YouTube 16:9** 1920×1080, **Shorts/Reels 9:16** 1080×1920, **Square 1:1** 1080×1080, **LinkedIn 4:5** 1080×1350.
- **Reposition/zoom** the source inside the preset (drag / scroll-wheel in the preview, 1–4×). Rotated phone footage uses its display size.
- Preview mirrors the export's crop math, fades and dissolve opacity; media WebKit cannot play (mkv, avi, mts…) raises a banner instead of freezing playback; **Space** play/pause, **←/→** frame nudge (**⇧** = 1 s), **Home/End**.
- **Help**: the **?** toolbar button (or the **?** key) opens a shortcuts dialog. `SHORTCUTS` in `src/components/HelpDialog.vue` is the single source; README's Keys table mirrors it.

Export (**⌘E**)
- **Stream copy** (`-c copy`, instant, keyframe-snapped) when a single untouched h264/hevc clip already matches the preset.
- Otherwise **hardware encode** via `h264_videotoolbox` (Draft 4 / Standard 10 / High 18 Mbit/s at 1080p, scaled by pixel count) + AAC 192k, `+faststart`. The dialog lists why a re-encode was needed.
- **Audio only** (.m4a AAC) for podcasts.
- Progress bar, elapsed time, cancel (kills ffmpeg and removes the partial file), Reveal in Finder.

Project
- New / Open / Save (**⌘S**, **⇧⌘S** save-as) as pretty JSON `.forgevideo`. Media paths inside the project folder are stored relative. Loading relayouts and clamps stale data. Unsaved-changes prompt on New/Open.
- Thumbnail filmstrips (≈40 JPEGs per source) and waveform peaks (1 byte / 10 ms) cached in `~/Library/Caches/ForgeVideo/<key>/`, keyed by path + size + mtime.

Not in V1 (do not add unless Nina reopens): recording (mic/camera/screen — `src-tauri/src/capture/` is a placeholder for Phase 3), proxies, titles/text, transitions on overlay layers, ffmpeg-next bindings, Metal.

## Layout

- `src/api/tauri.ts` is the **only** place that calls Rust. The UI never builds an ffmpeg argument.
- `src/types/project.ts` hand-mirrors `src-tauri/src/project/model.rs` — change both.
- `src/stores/project.ts` (Pinia) replaces its whole `Project` with what every mutating command returns.
- `src-tauri/src/`: `project` model + JSON persistence · `timeline` pure edit ops (`relayout` is the invariant keeper) · `media` ffprobe → `MediaInfo` · `render` planner (stream copy vs encode) + `graph` (filter_complex argv) + `ffmpeg` (spawn, progress, cancel) · `cache` thumbs/waveforms · `jobs` cancel registry + `job://*` events · `commands` thin `#[tauri::command]` layer.
- `src-tauri/vendor/wry/` is wry 0.55.1 with one guard in `src/wkwebview/drag_drop.rs` (upstream panics on drags that advertise filenames but carry none). Wired via `[patch.crates-io]`; drop it when upstream fixes the unwrap.

## Conventions

- Times are integer ms (`Ms`); rationals for fps; ffmpeg gets `ms_to_secs` strings (`1.250`).
- Every Rust op must leave the project valid: call `timeline::relayout` after any structural change.
- Tests: pure functions get direct unit tests; commands are exercised through `tauri::test::mock_app`; anything that spawns ffmpeg uses `tests/fixtures/` (`clip_a_720p.mp4`, `clip_b_1080p.mp4`, `music.m4a`). Frontend tests mock `src/api/tauri.ts` via `src/test/fixtures.ts` (`mockApi`, `resolveWith`) and stub browser APIs in `src/test/setup.ts`.
- Do not commit unless asked. Keep the app tiny; prefer removing options to adding them.
