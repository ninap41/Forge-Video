//! Runs real ffmpeg against the fixtures: stream-copy trim and a full multi-clip render.

use forge_video_lib::project::*;
use forge_video_lib::render::{plan, plan_with_texts, ffmpeg, ExportSettings, Quality, Strategy};
use forge_video_lib::{media, timeline};
use std::path::PathBuf;

fn fixture(n: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures").join(n)
}

async fn run(p: &Project, s: &ExportSettings) -> forge_video_lib::render::ExportPlan {
    let plan = plan(p, s).unwrap();
    let mut last = 0.0;
    ffmpeg::run_with_progress(&plan.args, plan.duration_ms, |f| last = f, None).await.unwrap();
    assert_eq!(last, 1.0);
    plan
}

#[tokio::test]
async fn stream_copy_trim_is_fast_and_valid() {
    let dir = tempfile::tempdir().unwrap();
    let src = fixture("clip_a_720p.mp4");
    let mut p = Project::new("t");
    p.aspect = AspectPreset::YouTube16x9;
    timeline::append(&mut p, Clip::new(src.clone(), media::probe(&src).await.unwrap()));
    let id = p.clips[0].id;
    timeline::trim(&mut p, id, 1000, 4000).unwrap();
    let out = dir.path().join("trim.mp4");
    let s = ExportSettings { destination: out.clone(), quality: Quality::Standard, audio_only: false };
    let plan = run(&p, &s).await;
    assert_eq!(plan.strategy, Strategy::StreamCopy);
    let m = media::probe(&out).await.unwrap();
    assert_eq!(m.codec, "h264");
    assert!((2500..=3600).contains(&m.duration_ms), "duration {}", m.duration_ms);
}

#[tokio::test]
async fn multi_clip_render_with_dissolve_music_and_vertical_crop() {
    let dir = tempfile::tempdir().unwrap();
    let a = fixture("clip_a_720p.mp4");
    let b = fixture("clip_b_1080p.mp4");
    let mut p = Project::new("t");
    timeline::append(&mut p, Clip::new(a.clone(), media::probe(&a).await.unwrap()));
    timeline::append(&mut p, Clip::new(b.clone(), media::probe(&b).await.unwrap()));
    let ida = p.clips[0].id;
    let idb = p.clips[1].id;
    timeline::trim(&mut p, ida, 0, 3000).unwrap();
    timeline::trim(&mut p, idb, 1000, 4000).unwrap();
    timeline::set_transition(&mut p, ida, Transition::CrossDissolve { ms: 500 }).unwrap();
    timeline::set_fades(&mut p, ida, 300, 0).unwrap();
    timeline::set_fades(&mut p, idb, 0, 300).unwrap();
    p.aspect = AspectPreset::Shorts9x16;
    p.crop = Crop { scale: 1.3, x: 0.4, y: 0.5 };
    let m = fixture("music.m4a");
    let t = timeline::audio_track_add(&mut p, "Music");
    let mid = timeline::audio_clip_add(&mut p, t, AudioClip::new(m.clone(), media::probe(&m).await.unwrap()), 0).unwrap();
    timeline::audio_clip_set(&mut p, mid, 0.5, 0, 1000, false).unwrap();
    assert_eq!(p.duration_ms(), 5500);

    let out = dir.path().join("render.mp4");
    let s = ExportSettings { destination: out.clone(), quality: Quality::Draft, audio_only: false };
    let plan = run(&p, &s).await;
    assert_eq!(plan.strategy, Strategy::HardwareEncode);
    let mi = media::probe(&out).await.unwrap();
    assert_eq!((mi.width, mi.height), (1080, 1920));
    assert_eq!(mi.codec, "h264");
    assert!(mi.has_audio);
    assert!((5300..=5700).contains(&mi.duration_ms), "duration {}", mi.duration_ms);

    // audio-only export of the same project
    let out_a = dir.path().join("render.m4a");
    let s = ExportSettings { destination: out_a.clone(), quality: Quality::Draft, audio_only: true };
    let plan = run(&p, &s).await;
    assert_eq!(plan.strategy, Strategy::AudioOnly);
    let ma = media::probe(&out_a).await.unwrap();
    assert_eq!(ma.width, 0);
    assert!(ma.has_audio);
    assert!((5300..=5700).contains(&ma.duration_ms), "duration {}", ma.duration_ms);
}

#[tokio::test]
async fn overlay_png_and_video_over_v1_with_two_audio_tracks() {
    let dir = tempfile::tempdir().unwrap();
    let a = fixture("clip_a_720p.mp4");
    let b = fixture("clip_b_1080p.mp4");
    let logo = fixture("logo.png");
    let m = fixture("music.m4a");
    let mut p = Project::new("t");
    timeline::append(&mut p, Clip::new(a.clone(), media::probe(&a).await.unwrap()));
    let ida = p.clips[0].id;
    timeline::trim(&mut p, ida, 0, 4000).unwrap();
    // PNG badge for 500..3000 with fades, then a picture-in-picture video 2000..3500
    let lo = timeline::overlay_add(&mut p, OverlayClip::new(logo.clone(), media::probe(&logo).await.unwrap()), 500, 0);
    timeline::overlay_trim(&mut p, lo, 0, 2500).unwrap();
    timeline::overlay_set_fades(&mut p, lo, 300, 300).unwrap();
    let vo = timeline::overlay_add(&mut p, OverlayClip::new(b.clone(), media::probe(&b).await.unwrap()), 3000, 1);
    timeline::overlay_trim(&mut p, vo, 1000, 2500).unwrap();
    timeline::overlay_set_placement(&mut p, vo, Placement { scale: 0.4, x: 0.2, y: 0.2 }).unwrap();
    assert_eq!(p.overlays.len(), 2);
    assert_eq!(p.overlay_layers, 2);
    // and a title card still on V1 in front of the footage
    timeline::append(&mut p, Clip::new(logo.clone(), media::probe(&logo).await.unwrap()));
    let card = p.clips[1].id;
    timeline::trim(&mut p, card, 0, 1000).unwrap();
    timeline::move_to(&mut p, card, 0).unwrap();
    let music = timeline::audio_track_add(&mut p, "Music");
    let sfx = timeline::audio_track_add(&mut p, "SFX");
    let mm = media::probe(&m).await.unwrap();
    timeline::audio_clip_add(&mut p, music, AudioClip::new(m.clone(), mm.clone()), 0).unwrap();
    let s1 = timeline::audio_clip_add(&mut p, sfx, AudioClip::new(m.clone(), mm), 3000).unwrap();
    timeline::audio_clip_trim(&mut p, s1, 0, 5000).unwrap();
    assert_eq!(p.duration_ms(), 5000);

    let out = dir.path().join("overlay.mp4");
    let s = ExportSettings { destination: out.clone(), quality: Quality::Draft, audio_only: false };
    let plan = run(&p, &s).await;
    assert_eq!(plan.strategy, Strategy::HardwareEncode);
    assert_eq!(plan.reasons, vec!["2 clips on the timeline"], "multiple V1 clips short-circuit the blocker list");
    let mi = media::probe(&out).await.unwrap();
    assert_eq!((mi.width, mi.height), (1920, 1080));
    assert!(mi.has_audio);
    assert!((4800..=5200).contains(&mi.duration_ms), "audio must not extend past V1: {}", mi.duration_ms);
}

#[tokio::test]
async fn text_raster_is_burned_in_above_the_overlay() {
    let dir = tempfile::tempdir().unwrap();
    let a = fixture("clip_a_720p.mp4");
    let logo = fixture("logo.png");
    let mut p = Project::new("t");
    timeline::append(&mut p, Clip::new(a.clone(), media::probe(&a).await.unwrap()));
    let ida = p.clips[0].id;
    timeline::trim(&mut p, ida, 0, 3000).unwrap();
    timeline::overlay_add(&mut p, OverlayClip::new(logo.clone(), media::probe(&logo).await.unwrap()), 0, 0);
    let mut t = TextClip::new("Hello");
    t.fade_in = 200; t.fade_out = 200;
    let tid = timeline::text_add(&mut p, t, 500);
    timeline::text_trim(&mut p, tid, 2000).unwrap();
    // the webview would rasterise the title; any PNG proves the path
    let raster = dir.path().join("title.png");
    std::fs::copy(&logo, &raster).unwrap();

    let out = dir.path().join("text.mp4");
    let s = ExportSettings { destination: out.clone(), quality: Quality::Draft, audio_only: false };
    let plan = plan_with_texts(&p, &s, &[(tid, raster)]).unwrap();
    assert!(plan.reasons.contains(&"text track".to_string()), "{:?}", plan.reasons);
    let mut last = 0.0;
    ffmpeg::run_with_progress(&plan.args, plan.duration_ms, |f| last = f, None).await.unwrap();
    assert_eq!(last, 1.0);
    let mi = media::probe(&out).await.unwrap();
    assert_eq!((mi.width, mi.height), (1920, 1080));
    assert!((2800..=3200).contains(&mi.duration_ms), "{}", mi.duration_ms);
}

/// Render a short synthetic file with ffmpeg; None when this ffmpeg build lacks the encoder.
async fn synth(dir: &std::path::Path, name: &str, args: &[&str]) -> Option<PathBuf> {
    let out = dir.join(name);
    let mut cmd = tokio::process::Command::new("ffmpeg");
    cmd.args(["-v", "error", "-y"]).args(args).arg(&out);
    let st = cmd.output().await.ok()?;
    if !st.status.success() {
        eprintln!("skipping {name}: {}", String::from_utf8_lossy(&st.stderr));
        return None;
    }
    Some(out)
}
const SINE: &[&str] = &["-f", "lavfi", "-i", "sine=frequency=440:sample_rate=44100:duration=3"];

#[tokio::test]
async fn many_audio_and_video_formats_probe_and_export() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let mp3 = synth(d, "a.mp3", &[SINE, &["-c:a", "libmp3lame"]].concat()).await;
    let wav = synth(d, "a.wav", &[SINE, &["-c:a", "pcm_s16le"]].concat()).await;
    let flac = synth(d, "a.flac", &[SINE, &["-c:a", "flac"]].concat()).await;
    let aiff = synth(d, "a.aiff", &[SINE, &["-c:a", "pcm_s16be"]].concat()).await;
    let logo = fixture("logo.png").to_string_lossy().to_string();
    let cover = synth(d, "cover.mp3", &[SINE, &["-i", &logo, "-map", "0:a", "-map", "1:v", "-c:a", "libmp3lame", "-c:v", "mjpeg", "-disposition:v", "attached_pic", "-id3v2_version", "3"]].concat()).await;
    let mkv = synth(d, "v.mkv", &["-f", "lavfi", "-i", "testsrc=size=640x360:rate=25:duration=3", "-f", "lavfi", "-i", "sine=frequency=220:duration=3", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac"]).await;
    let avi = synth(d, "v.avi", &["-f", "lavfi", "-i", "testsrc=size=320x240:rate=25:duration=2", "-c:v", "mpeg4"]).await;

    for a in [&mp3, &wav, &flac, &aiff, &cover].into_iter().flatten() {
        let m = media::probe(a).await.unwrap();
        assert_eq!(m.kind(), MediaKind::Audio, "{}", a.display());
        assert!(m.has_audio && !m.is_still, "{}", a.display());
        assert!((2900..=3200).contains(&m.duration_ms), "{} duration {}", a.display(), m.duration_ms);
    }
    for v in [&mkv, &avi].into_iter().flatten() {
        let m = media::probe(v).await.unwrap();
        assert_eq!(m.kind(), MediaKind::Video, "{}", v.display());
    }

    // V1 from the mkv (falls back to the fixture) with every synthesised audio clip layered on tracks.
    let v1 = mkv.clone().unwrap_or_else(|| fixture("clip_a_720p.mp4"));
    let mut p = Project::new("formats");
    timeline::append(&mut p, Clip::new(v1.clone(), media::probe(&v1).await.unwrap()));
    if let Some(avi) = &avi {
        timeline::append(&mut p, Clip::new(avi.clone(), media::probe(avi).await.unwrap()));
    }
    let t = timeline::audio_track_add(&mut p, "Music");
    let mut at = 0;
    for a in [&mp3, &wav, &flac, &aiff, &cover].into_iter().flatten() {
        timeline::audio_clip_add(&mut p, t, AudioClip::new(a.clone(), media::probe(a).await.unwrap()), at).unwrap();
        at += 500;
    }
    let out = d.join("mix.mp4");
    let s = ExportSettings { destination: out.clone(), quality: Quality::Draft, audio_only: false };
    let plan = run(&p, &s).await;
    assert_eq!(plan.strategy, Strategy::HardwareEncode);
    let m = media::probe(&out).await.unwrap();
    assert_eq!(m.codec, "h264");
    assert_eq!(m.audio_codec.as_deref(), Some("aac"));
    assert_eq!(m.duration_ms / 100, p.duration_ms() / 100);

    let out_a = d.join("mix.m4a");
    let s = ExportSettings { destination: out_a.clone(), quality: Quality::Draft, audio_only: true };
    run(&p, &s).await;
    assert_eq!(media::probe(&out_a).await.unwrap().kind(), MediaKind::Audio);
}
