//! Runs real ffmpeg against the fixtures: stream-copy trim and a full multi-clip render.

use forge_video_lib::project::*;
use forge_video_lib::render::{plan, ffmpeg, ExportSettings, Quality, Strategy};
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
    let mut music = AudioTrack::new(m.clone(), media::probe(&m).await.unwrap().duration_ms);
    music.fade_out = 1000;
    p.music = Some(music);
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
