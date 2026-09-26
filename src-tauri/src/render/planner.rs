//! Decides how to export: copy compressed packets (instant) or build a filter graph and encode.

use crate::project::{AspectPreset, Project};
use crate::render::ffmpeg::ms_to_secs;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Quality {
    Draft,
    Standard,
    High,
}

impl Quality {
    /// Video bitrate for VideoToolbox, scaled by output pixel count relative to 1080p.
    pub fn bitrate_kbps(&self, w: u32, h: u32) -> u32 {
        let base = match self {
            Quality::Draft => 4_000,
            Quality::Standard => 10_000,
            Quality::High => 18_000,
        } as f64;
        let px_ratio = (w as f64 * h as f64) / (1920.0 * 1080.0);
        (base * px_ratio.max(0.25)).round() as u32
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportSettings {
    pub destination: PathBuf,
    pub quality: Quality,
    /// Audio-only export (m4a). Skips video entirely.
    pub audio_only: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Strategy {
    /// Single untouched clip: `-c copy`. Cut points snap to keyframes.
    StreamCopy,
    /// Filter graph + h264_videotoolbox.
    HardwareEncode,
    AudioOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportPlan {
    pub strategy: Strategy,
    pub duration_ms: u64,
    pub output: (u32, u32),
    pub destination: PathBuf,
    /// Why stream copy was not possible (for the UI tooltip).
    pub reasons: Vec<String>,
    pub args: Vec<String>,
}

fn source_matches_preset(p: &Project, preset: AspectPreset) -> bool {
    let c = &p.clips[0];
    let (w, h) = c.media.display_size();
    let (pw, ph) = preset.dimensions();
    if w == 0 || h == 0 { return false; }
    // Same aspect ratio within 1% and no upscaling needed beyond the source itself.
    ((w as f64 / h as f64) - (pw as f64 / ph as f64)).abs() < 0.01
}

pub fn stream_copy_blockers(p: &Project) -> Vec<String> {
    let mut r = Vec::new();
    if p.clips.len() != 1 {
        r.push(format!("{} clips on the timeline", p.clips.len()));
        return r;
    }
    let c = &p.clips[0];
    if c.fade_in > 0 || c.fade_out > 0 { r.push("fades".into()); }
    if p.music.is_some() { r.push("music track".into()); }
    if !p.crop.is_identity() { r.push("crop / reposition".into()); }
    if !source_matches_preset(p, p.aspect) { r.push("aspect ratio change".into()); }
    if c.codec_is_copyable() == false { r.push(format!("source codec {}", c.media.codec)); }
    if c.muted || (c.volume - 1.0).abs() > 1e-3 { r.push("volume change".into()); }
    if c.media.has_audio && c.media.audio_codec.as_deref() != Some("aac") { r.push("audio codec".into()); }
    r
}

impl crate::project::Clip {
    pub fn codec_is_copyable(&self) -> bool {
        matches!(self.media.codec.as_str(), "h264" | "hevc")
    }
}

pub fn plan(p: &Project, s: &ExportSettings) -> crate::error::Result<ExportPlan> {
    if p.clips.is_empty() {
        return Err(crate::error::Error::Export("timeline is empty".into()));
    }
    let duration_ms = p.duration_ms();
    if s.audio_only {
        return Ok(ExportPlan {
            strategy: Strategy::AudioOnly,
            duration_ms,
            output: (0, 0),
            destination: s.destination.clone(),
            reasons: vec![],
            args: crate::render::graph::build_args(p, s),
        });
    }
    let reasons = stream_copy_blockers(p);
    if reasons.is_empty() {
        let c = &p.clips[0];
        let args = vec![
            "-y".into(),
            "-ss".into(), ms_to_secs(c.source_start),
            "-to".into(), ms_to_secs(c.source_end),
            "-i".into(), c.source.to_string_lossy().into(),
            "-c".into(), "copy".into(),
            "-map".into(), "0:v:0".into(), "-map".into(), "0:a?".into(),
            "-movflags".into(), "+faststart".into(),
            s.destination.to_string_lossy().into(),
        ];
        return Ok(ExportPlan {
            strategy: Strategy::StreamCopy,
            duration_ms,
            output: c.media.display_size(),
            destination: s.destination.clone(),
            reasons,
            args,
        });
    }
    Ok(ExportPlan {
        strategy: Strategy::HardwareEncode,
        duration_ms,
        output: p.aspect.dimensions(),
        destination: s.destination.clone(),
        reasons,
        args: crate::render::graph::build_args(p, s),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::*;
    use crate::timeline;
    use std::path::PathBuf;

    fn media() -> MediaInfo {
        MediaInfo {
            duration_ms: 5000, width: 1920, height: 1080, fps: Rational { num: 30, den: 1 },
            codec: "h264".into(), container: "mov,mp4".into(), has_audio: true,
            audio_codec: Some("aac".into()), sample_rate: Some(48000), rotation: 0,
        }
    }
    fn settings() -> ExportSettings {
        ExportSettings { destination: PathBuf::from("/tmp/out.mp4"), quality: Quality::Standard, audio_only: false }
    }

    #[test]
    fn single_untouched_clip_is_stream_copy() {
        let mut p = Project::new("t");
        timeline::append(&mut p, Clip::new(PathBuf::from("/a.mp4"), media()));
        let id = p.clips[0].id;
        timeline::trim(&mut p, id, 1000, 4000).unwrap();
        let plan = plan(&p, &settings()).unwrap();
        assert_eq!(plan.strategy, Strategy::StreamCopy);
        assert!(plan.args.windows(2).any(|w| w[0] == "-c" && w[1] == "copy"));
        assert!(plan.args.contains(&"1.000".to_string()));
        assert!(plan.args.contains(&"4.000".to_string()));
    }

    #[test]
    fn edits_force_encode_with_reasons() {
        let mut p = Project::new("t");
        timeline::append(&mut p, Clip::new(PathBuf::from("/a.mp4"), media()));
        let id = p.clips[0].id;
        timeline::set_fades(&mut p, id, 500, 0).unwrap();
        p.aspect = AspectPreset::Shorts9x16;
        let plan = plan(&p, &settings()).unwrap();
        assert_eq!(plan.strategy, Strategy::HardwareEncode);
        assert!(plan.reasons.iter().any(|r| r.contains("fades")));
        assert!(plan.reasons.iter().any(|r| r.contains("aspect")));
        assert!(plan.args.contains(&"h264_videotoolbox".to_string()));
        assert_eq!(plan.output, (1080, 1920));
    }

    #[test]
    fn two_clips_force_encode() {
        let mut p = Project::new("t");
        timeline::append(&mut p, Clip::new(PathBuf::from("/a.mp4"), media()));
        timeline::append(&mut p, Clip::new(PathBuf::from("/b.mp4"), media()));
        assert_eq!(plan(&p, &settings()).unwrap().strategy, Strategy::HardwareEncode);
    }

    #[test]
    fn empty_timeline_errors() {
        assert!(plan(&Project::new("e"), &settings()).is_err());
    }
}

#[cfg(test)]
mod more_tests {
    use super::*;
    use crate::project::*;
    use crate::timeline;
    use std::path::PathBuf;

    fn media(w: u32, h: u32) -> MediaInfo {
        MediaInfo {
            duration_ms: 5000, width: w, height: h, fps: Rational { num: 30, den: 1 },
            codec: "h264".into(), container: "mov,mp4".into(), has_audio: true,
            audio_codec: Some("aac".into()), sample_rate: Some(48000), rotation: 0,
        }
    }
    fn one(m: MediaInfo) -> Project {
        let mut p = Project::new("t");
        timeline::append(&mut p, Clip::new(PathBuf::from("/a.mp4"), m));
        p
    }
    fn settings() -> ExportSettings {
        ExportSettings { destination: PathBuf::from("/tmp/out.mp4"), quality: Quality::Standard, audio_only: false }
    }

    #[test]
    fn bitrate_scales_with_pixel_count_and_has_a_floor() {
        assert_eq!(Quality::Draft.bitrate_kbps(1920, 1080), 4_000);
        assert_eq!(Quality::Standard.bitrate_kbps(1920, 1080), 10_000);
        assert_eq!(Quality::High.bitrate_kbps(1920, 1080), 18_000);
        assert_eq!(Quality::Standard.bitrate_kbps(1080, 1920), 10_000, "vertical has the same pixel count");
        assert_eq!(Quality::Standard.bitrate_kbps(1080, 1080), 5_625);
        assert_eq!(Quality::Standard.bitrate_kbps(320, 240), 2_500, "clamped at a quarter of 1080p");
        assert_eq!(Quality::High.bitrate_kbps(3840, 2160), 72_000);
    }

    #[test]
    fn codec_copyability() {
        let p = one(media(1920, 1080));
        assert!(p.clips[0].codec_is_copyable());
        let mut m = media(1920, 1080);
        m.codec = "hevc".into();
        assert!(one(m).clips[0].codec_is_copyable());
        for c in ["prores", "vp9", "av1", "mpeg4", ""] {
            let mut m = media(1920, 1080);
            m.codec = c.into();
            assert!(!one(m).clips[0].codec_is_copyable(), "{c}");
        }
    }

    #[test]
    fn each_blocker_is_reported_individually() {
        let p = one(media(1920, 1080));
        assert!(stream_copy_blockers(&p).is_empty());

        let mut p2 = p.clone();
        p2.music = Some(AudioTrack::new(PathBuf::from("/m.m4a"), 1000));
        assert_eq!(stream_copy_blockers(&p2), vec!["music track"]);

        let mut p2 = p.clone();
        p2.crop = Crop { scale: 1.5, x: 0.5, y: 0.5 };
        assert_eq!(stream_copy_blockers(&p2), vec!["crop / reposition"]);

        let mut p2 = p.clone();
        p2.aspect = AspectPreset::Square1x1;
        assert_eq!(stream_copy_blockers(&p2), vec!["aspect ratio change"]);

        let mut p2 = p.clone();
        p2.clips[0].media.codec = "prores".into();
        assert_eq!(stream_copy_blockers(&p2), vec!["source codec prores"]);

        let mut p2 = p.clone();
        p2.clips[0].volume = 0.5;
        assert_eq!(stream_copy_blockers(&p2), vec!["volume change"]);
        let mut p2 = p.clone();
        p2.clips[0].muted = true;
        assert_eq!(stream_copy_blockers(&p2), vec!["volume change"]);

        let mut p2 = p.clone();
        p2.clips[0].media.audio_codec = Some("pcm_s16le".into());
        assert_eq!(stream_copy_blockers(&p2), vec!["audio codec"]);

        let mut p2 = p.clone();
        p2.clips[0].fade_out = 200;
        assert_eq!(stream_copy_blockers(&p2), vec!["fades"]);
    }

    #[test]
    fn silent_source_does_not_trip_the_audio_codec_check() {
        let mut m = media(1920, 1080);
        m.has_audio = false;
        m.audio_codec = None;
        let p = one(m);
        assert!(stream_copy_blockers(&p).is_empty());
        assert_eq!(plan(&p, &settings()).unwrap().strategy, Strategy::StreamCopy);
    }

    #[test]
    fn multiple_clips_short_circuit_with_a_count() {
        let mut p = one(media(1920, 1080));
        timeline::append(&mut p, Clip::new(PathBuf::from("/b.mp4"), media(1920, 1080)));
        p.music = Some(AudioTrack::new(PathBuf::from("/m.m4a"), 1000));
        assert_eq!(stream_copy_blockers(&p), vec!["2 clips on the timeline"]);
    }

    #[test]
    fn rotated_phone_footage_matches_vertical_preset() {
        let mut m = media(1920, 1080);
        m.rotation = 90;
        let mut p = one(m);
        p.aspect = AspectPreset::Shorts9x16;
        let plan = plan(&p, &settings()).unwrap();
        assert_eq!(plan.strategy, Strategy::StreamCopy);
        assert_eq!(plan.output, (1080, 1920), "reports display size");
    }

    #[test]
    fn zero_sized_source_can_never_stream_copy() {
        let p = one(media(0, 0));
        assert!(stream_copy_blockers(&p).contains(&"aspect ratio change".to_string()));
    }

    #[test]
    fn stream_copy_plan_uses_source_range_and_faststart() {
        let mut p = one(media(1920, 1080));
        let id = p.clips[0].id;
        timeline::trim(&mut p, id, 250, 4750).unwrap();
        let plan = plan(&p, &settings()).unwrap();
        assert_eq!(plan.duration_ms, 4500);
        assert_eq!(plan.destination, PathBuf::from("/tmp/out.mp4"));
        assert!(plan.reasons.is_empty());
        let a = &plan.args;
        assert_eq!(a[0], "-y");
        assert_eq!(&a[1..5], &["-ss", "0.250", "-to", "4.750"]);
        assert_eq!(a[a.len() - 1], "/tmp/out.mp4");
        assert!(a.windows(2).any(|w| w[0] == "-movflags" && w[1] == "+faststart"));
        assert!(a.windows(2).any(|w| w[0] == "-map" && w[1] == "0:a?"), "audio is optional");
        assert!(!a.iter().any(|x| x.contains("videotoolbox")));
    }

    #[test]
    fn audio_only_plan_skips_stream_copy_even_for_a_clean_clip() {
        let p = one(media(1920, 1080));
        let mut s = settings();
        s.audio_only = true;
        s.destination = PathBuf::from("/tmp/pod.m4a");
        let plan = plan(&p, &s).unwrap();
        assert_eq!(plan.strategy, Strategy::AudioOnly);
        assert_eq!(plan.output, (0, 0));
        assert_eq!(plan.duration_ms, 5000);
        assert!(plan.args.contains(&"[aout]".to_string()));
        assert!(!plan.args.contains(&"[vout]".to_string()));
        assert_eq!(plan.args.last().unwrap(), "/tmp/pod.m4a");
    }

    #[test]
    fn encode_plan_duration_accounts_for_transitions() {
        let mut p = one(media(1920, 1080));
        timeline::append(&mut p, Clip::new(PathBuf::from("/b.mp4"), media(1920, 1080)));
        let a = p.clips[0].id;
        timeline::set_transition(&mut p, a, Transition::CrossDissolve { ms: 1000 }).unwrap();
        let plan = plan(&p, &settings()).unwrap();
        assert_eq!(plan.duration_ms, 9000);
        assert_eq!(plan.output, (1920, 1080));
    }

    #[test]
    fn quality_and_settings_serialize_for_the_frontend() {
        assert_eq!(serde_json::to_string(&Quality::High).unwrap(), "\"High\"");
        assert_eq!(serde_json::to_string(&Strategy::StreamCopy).unwrap(), "\"StreamCopy\"");
        let s: ExportSettings = serde_json::from_str(r#"{"destination":"/x.mp4","quality":"Draft","audio_only":true}"#).unwrap();
        assert_eq!(s.quality, Quality::Draft);
        assert!(s.audio_only);
        assert_eq!(s.destination, PathBuf::from("/x.mp4"));
    }
}
