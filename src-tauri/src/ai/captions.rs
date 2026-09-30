//! Captions are derived: source-time cues mapped through the V1 clips. Pure.

use crate::project::{Cue, Ms, Project};
use crate::timeline::MIN_CLIP_MS;

/// Every cue as it falls on the timeline, in order. A cue cut by a trim is shortened, not dropped.
pub fn timeline_cues(p: &Project) -> Vec<Cue> {
    let mut out = Vec::new();
    for c in p.clips.iter().filter(|c| !c.media.is_still) {
        let Some(t) = p.transcripts.iter().find(|t| t.source == c.source) else { continue };
        for cue in &t.cues {
            let start = cue.start.max(c.source_start);
            let end = cue.end.min(c.source_end);
            if start + MIN_CLIP_MS > end {
                continue;
            }
            out.push(Cue {
                id: cue.id,
                start: c.timeline_start + (start - c.source_start),
                end: c.timeline_start + (end - c.source_start),
                text: cue.text.clone(),
            });
        }
    }
    out.sort_by_key(|c| c.start);
    out
}

fn srt_time(ms: Ms) -> String {
    format!("{:02}:{:02}:{:02},{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000)
}

pub fn to_srt(cues: &[Cue]) -> String {
    cues.iter()
        .enumerate()
        .map(|(i, c)| format!("{}\n{} --> {}\n{}\n", i + 1, srt_time(c.start), srt_time(c.end), c.text.trim()))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::project::{Clip, MediaInfo, Rational, Transcript};
    use crate::timeline;
    use std::path::PathBuf;
    use uuid::Uuid;

    pub fn media(ms: Ms) -> MediaInfo {
        MediaInfo {
            duration_ms: ms, width: 1920, height: 1080, fps: Rational { num: 30, den: 1 },
            codec: "h264".into(), container: "mov,mp4".into(), has_audio: true,
            audio_codec: Some("aac".into()), sample_rate: Some(48000), rotation: 0, is_still: false,
        }
    }

    pub fn cue(start: Ms, end: Ms, text: &str) -> Cue {
        Cue { id: Uuid::new_v4(), start, end, text: text.into() }
    }

    /// Two sources, a.mp4 (60 s) and b.mp4 (30 s), each with a cue every 10 s.
    pub fn podcast() -> Project {
        let mut p = Project::new("pod");
        for (name, len) in [("/a.mp4", 60_000), ("/b.mp4", 30_000)] {
            timeline::append(&mut p, Clip::new(PathBuf::from(name), media(len)));
            let cues = (0..len / 10_000).map(|i| cue(i * 10_000, i * 10_000 + 9_000, &format!("{name} {i}"))).collect();
            p.transcripts.push(Transcript { source: PathBuf::from(name), cues });
        }
        p
    }

    fn spans(p: &Project) -> Vec<(Ms, Ms, String)> {
        timeline_cues(p).into_iter().map(|c| (c.start, c.end, c.text)).collect()
    }

    #[test]
    fn cues_follow_trims_splits_and_reorders() {
        let mut p = podcast();
        let all = spans(&p);
        assert_eq!(all.len(), 9);
        assert_eq!(all[0], (0, 9_000, "/a.mp4 0".into()));
        assert_eq!(all[6], (60_000, 69_000, "/b.mp4 0".into()), "second clip is offset by the first");

        let (a, b) = (p.clips[0].id, p.clips[1].id);
        timeline::trim(&mut p, a, 15_000, 32_000).unwrap();
        let t = spans(&p);
        assert_eq!(t[0], (0, 4_000, "/a.mp4 1".into()), "a cue cut by the in point is shortened");
        assert_eq!(t[1], (5_000, 14_000, "/a.mp4 2".into()));
        assert_eq!(t[2], (15_000, 17_000, "/a.mp4 3".into()), "and by the out point");
        assert_eq!(t[3], (17_000, 26_000, "/b.mp4 0".into()));

        timeline::move_to(&mut p, b, 0).unwrap();
        let t = spans(&p);
        assert_eq!(t[0], (0, 9_000, "/b.mp4 0".into()));
        assert_eq!(t[3], (30_000, 34_000, "/a.mp4 1".into()));

        timeline::split(&mut p, b, 5_000).unwrap();
        let t = spans(&p);
        assert_eq!((t[0].0, t[0].1), (0, 5_000));
        assert_eq!((t[1].0, t[1].1, t[1].2.as_str()), (5_000, 9_000, "/b.mp4 0"), "a split cue shows on both halves");
    }

    #[test]
    fn stills_and_sources_without_a_transcript_have_no_cues() {
        let mut p = podcast();
        p.transcripts.remove(0);
        assert_eq!(timeline_cues(&p).len(), 3);
        p.clips[1].media.is_still = true;
        assert!(timeline_cues(&p).is_empty());
        assert!(timeline_cues(&Project::new("e")).is_empty());
    }

    #[test]
    fn srt_is_numbered_and_uses_comma_milliseconds() {
        let srt = to_srt(&[cue(0, 1_500, " Hello there "), cue(3_723_004, 3_725_000, "Bye")]);
        assert_eq!(srt, "1\n00:00:00,000 --> 00:00:01,500\nHello there\n\n2\n01:02:03,004 --> 01:02:05,000\nBye\n");
        assert_eq!(to_srt(&[]), "");
    }
}
