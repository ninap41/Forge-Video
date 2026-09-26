//! `ffprobe` wrapper: turns a media file into `MediaInfo`.

use crate::error::{Error, Result};
use crate::project::{MediaInfo, Rational};
use crate::render::ffmpeg::ffprobe_bin;
use serde::Deserialize;
use std::path::Path;
use tokio::process::Command;

#[derive(Deserialize)]
struct ProbeOut {
    #[serde(default)]
    streams: Vec<Stream>,
    format: Option<Format>,
}
#[derive(Deserialize)]
struct Format {
    format_name: Option<String>,
    duration: Option<String>,
}
#[derive(Deserialize)]
struct Stream {
    codec_type: Option<String>,
    codec_name: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    r_frame_rate: Option<String>,
    avg_frame_rate: Option<String>,
    sample_rate: Option<String>,
    duration: Option<String>,
    #[serde(default)]
    tags: serde_json::Map<String, serde_json::Value>,
    #[serde(default)]
    side_data_list: Vec<serde_json::Value>,
}

fn parse_rational(s: &str) -> Option<Rational> {
    let (n, d) = s.split_once('/')?;
    let num: u32 = n.trim().parse().ok()?;
    let den: u32 = d.trim().parse().ok()?;
    if den == 0 { None } else { Some(Rational { num, den }) }
}

fn parse_secs_to_ms(s: &str) -> Option<u64> {
    s.parse::<f64>().ok().map(|f| (f * 1000.0).round() as u64)
}

fn rotation_of(s: &Stream) -> i32 {
    let mut r = s.tags.get("rotate").and_then(|v| v.as_str()).and_then(|v| v.parse::<i32>().ok());
    if r.is_none() {
        r = s.side_data_list.iter().find_map(|sd| sd.get("rotation").and_then(|v| v.as_f64()).map(|v| v.round() as i32));
    }
    ((r.unwrap_or(0) % 360) + 360) % 360
}

pub fn parse_probe_json(json: &str) -> Result<MediaInfo> {
    let out: ProbeOut = serde_json::from_str(json)?;
    let video = out.streams.iter().find(|s| s.codec_type.as_deref() == Some("video"));
    let audio = out.streams.iter().find(|s| s.codec_type.as_deref() == Some("audio"));
    let fmt = out.format.as_ref();
    let duration_ms = fmt
        .and_then(|f| f.duration.as_deref())
        .and_then(parse_secs_to_ms)
        .or_else(|| video.or(audio).and_then(|s| s.duration.as_deref()).and_then(parse_secs_to_ms))
        .ok_or_else(|| Error::Media("no duration".into()))?;
    if video.is_none() && audio.is_none() {
        return Err(Error::Media("no video or audio stream".into()));
    }
    let fps = video
        .and_then(|v| v.avg_frame_rate.as_deref().and_then(parse_rational).filter(|r| r.num > 0))
        .or_else(|| video.and_then(|v| v.r_frame_rate.as_deref().and_then(parse_rational)))
        .unwrap_or(Rational { num: 0, den: 1 });
    Ok(MediaInfo {
        duration_ms,
        width: video.and_then(|v| v.width).unwrap_or(0),
        height: video.and_then(|v| v.height).unwrap_or(0),
        fps,
        codec: video.and_then(|v| v.codec_name.clone()).unwrap_or_default(),
        container: fmt.and_then(|f| f.format_name.clone()).unwrap_or_default(),
        has_audio: audio.is_some(),
        audio_codec: audio.and_then(|a| a.codec_name.clone()),
        sample_rate: audio.and_then(|a| a.sample_rate.as_deref()).and_then(|s| s.parse().ok()),
        rotation: video.map(rotation_of).unwrap_or(0),
    })
}

pub async fn probe(path: &Path) -> Result<MediaInfo> {
    let out = Command::new(ffprobe_bin()?)
        .args(["-v", "error", "-print_format", "json", "-show_streams", "-show_format"])
        .arg(path)
        .output()
        .await?;
    if !out.status.success() {
        return Err(Error::Media(String::from_utf8_lossy(&out.stderr).trim().to_string()));
    }
    parse_probe_json(&String::from_utf8_lossy(&out.stdout))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn probes_fixture() {
        let p = crate::test_util::fixture("clip_a_720p.mp4");
        let m = probe(&p).await.unwrap();
        assert_eq!((m.width, m.height), (1280, 720));
        assert_eq!(m.fps, Rational { num: 30, den: 1 });
        assert!((4900..=5100).contains(&m.duration_ms), "duration {}", m.duration_ms);
        assert_eq!(m.codec, "h264");
        assert!(m.has_audio);
        assert_eq!(m.audio_codec.as_deref(), Some("aac"));
        assert_eq!(m.rotation, 0);
    }

    #[test]
    fn parses_rotation_from_tags_and_side_data() {
        let j = r#"{"streams":[{"codec_type":"video","codec_name":"hevc","width":1920,"height":1080,
            "avg_frame_rate":"60000/1001","tags":{"rotate":"90"}}],"format":{"format_name":"mov","duration":"2.5"}}"#;
        let m = parse_probe_json(j).unwrap();
        assert_eq!(m.rotation, 90);
        assert_eq!(m.display_size(), (1080, 1920));
        assert_eq!(m.duration_ms, 2500);
        let j = r#"{"streams":[{"codec_type":"video","width":100,"height":50,"r_frame_rate":"25/1",
            "side_data_list":[{"rotation":-90}]}],"format":{"duration":"1"}}"#;
        assert_eq!(parse_probe_json(j).unwrap().rotation, 270);
    }
}

#[cfg(test)]
mod more_tests {
    use super::*;

    #[test]
    fn rational_parsing_rejects_garbage_and_zero_denominators() {
        assert_eq!(parse_rational("30/1"), Some(Rational { num: 30, den: 1 }));
        assert_eq!(parse_rational(" 30000 / 1001 "), Some(Rational { num: 30000, den: 1001 }));
        assert_eq!(parse_rational("0/0"), None);
        assert_eq!(parse_rational("30"), None);
        assert_eq!(parse_rational("a/b"), None);
        assert_eq!(parse_rational("-1/1"), None);
    }

    #[test]
    fn seconds_to_ms_rounds() {
        assert_eq!(parse_secs_to_ms("2.5"), Some(2500));
        assert_eq!(parse_secs_to_ms("0.0004"), Some(0));
        assert_eq!(parse_secs_to_ms("0.0005"), Some(1));
        assert_eq!(parse_secs_to_ms("5.016667"), Some(5017));
        assert_eq!(parse_secs_to_ms("N/A"), None);
    }

    #[test]
    fn audio_only_file_has_no_video_dimensions() {
        let j = r#"{"streams":[{"codec_type":"audio","codec_name":"aac","sample_rate":"44100"}],
            "format":{"format_name":"mov,mp4,m4a","duration":"12.0"}}"#;
        let m = parse_probe_json(j).unwrap();
        assert_eq!((m.width, m.height), (0, 0));
        assert_eq!(m.codec, "");
        assert_eq!(m.fps, Rational { num: 0, den: 1 });
        assert!(m.has_audio);
        assert_eq!(m.audio_codec.as_deref(), Some("aac"));
        assert_eq!(m.sample_rate, Some(44100));
        assert_eq!(m.duration_ms, 12_000);
        assert_eq!(m.container, "mov,mp4,m4a");
        assert_eq!(m.rotation, 0);
    }

    #[test]
    fn silent_video_has_no_audio_fields() {
        let j = r#"{"streams":[{"codec_type":"video","codec_name":"h264","width":640,"height":480,"avg_frame_rate":"25/1"}],
            "format":{"duration":"3"}}"#;
        let m = parse_probe_json(j).unwrap();
        assert!(!m.has_audio);
        assert_eq!(m.audio_codec, None);
        assert_eq!(m.sample_rate, None);
        assert_eq!(m.fps, Rational { num: 25, den: 1 });
    }

    #[test]
    fn duration_falls_back_to_stream_when_format_lacks_it() {
        let j = r#"{"streams":[{"codec_type":"video","width":10,"height":10,"duration":"1.25","r_frame_rate":"24/1"}],"format":{}}"#;
        assert_eq!(parse_probe_json(j).unwrap().duration_ms, 1250);
        let j = r#"{"streams":[{"codec_type":"audio","duration":"0.5"}]}"#;
        assert_eq!(parse_probe_json(j).unwrap().duration_ms, 500);
    }

    #[test]
    fn errors_for_missing_duration_or_streams() {
        let j = r#"{"streams":[{"codec_type":"video","width":10,"height":10}],"format":{}}"#;
        assert!(matches!(parse_probe_json(j), Err(Error::Media(m)) if m.contains("duration")));
        let j = r#"{"streams":[{"codec_type":"data"}],"format":{"duration":"1"}}"#;
        assert!(matches!(parse_probe_json(j), Err(Error::Media(m)) if m.contains("no video or audio")));
        assert!(matches!(parse_probe_json("not json"), Err(Error::Json(_))));
        assert!(matches!(parse_probe_json("{}"), Err(Error::Media(_))));
    }

    #[test]
    fn fps_prefers_avg_frame_rate_but_skips_zero() {
        let j = r#"{"streams":[{"codec_type":"video","width":10,"height":10,"avg_frame_rate":"0/0","r_frame_rate":"50/1"}],"format":{"duration":"1"}}"#;
        assert_eq!(parse_probe_json(j).unwrap().fps, Rational { num: 50, den: 1 });
        let j = r#"{"streams":[{"codec_type":"video","width":10,"height":10,"avg_frame_rate":"24000/1001","r_frame_rate":"50/1"}],"format":{"duration":"1"}}"#;
        assert_eq!(parse_probe_json(j).unwrap().fps, Rational { num: 24000, den: 1001 });
    }

    #[test]
    fn rotation_is_normalised_and_tags_win_over_side_data() {
        let mk = |tags: &str, sd: &str| format!(
            r#"{{"streams":[{{"codec_type":"video","width":10,"height":10,"tags":{{{tags}}},"side_data_list":[{sd}]}}],"format":{{"duration":"1"}}}}"#
        );
        assert_eq!(parse_probe_json(&mk(r#""rotate":"180""#, "")).unwrap().rotation, 180);
        assert_eq!(parse_probe_json(&mk(r#""rotate":"450""#, "")).unwrap().rotation, 90);
        assert_eq!(parse_probe_json(&mk(r#""rotate":"-270""#, "")).unwrap().rotation, 90);
        assert_eq!(parse_probe_json(&mk(r#""rotate":"junk""#, r#"{"rotation":90}"#)).unwrap().rotation, 90);
        assert_eq!(parse_probe_json(&mk(r#""rotate":"90""#, r#"{"rotation":-90}"#)).unwrap().rotation, 90);
        assert_eq!(parse_probe_json(&mk("", r#"{"rotation":-90.0}"#)).unwrap().rotation, 270);
        assert_eq!(parse_probe_json(&mk("", r#"{"something_else":1}"#)).unwrap().rotation, 0);
    }

    #[test]
    fn first_video_and_audio_streams_are_used() {
        let j = r#"{"streams":[
            {"codec_type":"audio","codec_name":"mp3","sample_rate":"48000"},
            {"codec_type":"video","codec_name":"hevc","width":3840,"height":2160,"avg_frame_rate":"60/1"},
            {"codec_type":"video","codec_name":"mjpeg","width":100,"height":100},
            {"codec_type":"audio","codec_name":"aac"}],
            "format":{"format_name":"matroska,webm","duration":"9.999"}}"#;
        let m = parse_probe_json(j).unwrap();
        assert_eq!(m.codec, "hevc");
        assert_eq!((m.width, m.height), (3840, 2160));
        assert_eq!(m.audio_codec.as_deref(), Some("mp3"));
        assert_eq!(m.duration_ms, 9999);
    }

    #[tokio::test]
    async fn probes_the_1080p_fixture_and_the_music() {
        let m = probe(&crate::test_util::fixture("clip_b_1080p.mp4")).await.unwrap();
        assert_eq!((m.width, m.height), (1920, 1080));
        assert_eq!(m.codec, "h264");
        let m = probe(&crate::test_util::fixture("music.m4a")).await.unwrap();
        assert_eq!(m.width, 0);
        assert!(m.has_audio);
        assert_eq!(m.audio_codec.as_deref(), Some("aac"));
        assert!(m.duration_ms > 1000);
    }

    #[tokio::test]
    async fn probing_a_missing_or_non_media_file_errors() {
        assert!(matches!(probe(std::path::Path::new("/no/such/file.mp4")).await, Err(Error::Media(_))));
        let dir = tempfile::tempdir().unwrap();
        let txt = dir.path().join("notes.txt");
        std::fs::write(&txt, "hello").unwrap();
        assert!(probe(&txt).await.is_err());
    }
}
