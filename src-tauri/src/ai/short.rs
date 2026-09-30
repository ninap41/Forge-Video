//! Applying a highlight's plan: a new 9:16 project holding only the kept ranges. Pure, apart
//! from `free_file` looking at the disk.

use crate::error::{Error, Result};
use crate::project::{AspectPreset, Clip, Highlight, PoolItem, Project, Transition};
use crate::timeline::{self, MIN_CLIP_MS};
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// The short for `h`, cut from `p`'s V1 track. Overlays and audio tracks are not carried over.
pub fn build_short(p: &Project, h: &Highlight) -> Result<Project> {
    let mut s = Project::new(h.title.clone());
    s.aspect = AspectPreset::Shorts9x16;
    s.video_muted = p.video_muted;
    s.fps = p.fps;
    for r in &h.keep {
        for c in &p.clips {
            let (cs, ce) = (c.timeline_start, c.timeline_start + c.duration_ms());
            let (a, b) = (r.start.max(cs), r.end.min(ce));
            if a + MIN_CLIP_MS > b {
                continue;
            }
            let mut piece: Clip = c.clone();
            piece.id = Uuid::new_v4();
            piece.source_start = c.source_start + (a - cs);
            piece.source_end = c.source_start + (b - cs);
            piece.fade_in = 0;
            piece.fade_out = 0;
            piece.transition_out = Transition::None;
            s.clips.push(piece);
        }
    }
    let Some(last) = s.clips.len().checked_sub(1) else {
        return Err(Error::InvalidEdit("the highlight no longer covers any clip".into()));
    };
    s.clips[0].fade_in = h.fade_in;
    s.clips[last].fade_out = h.fade_out;
    let used = |path: &Path| s.clips.iter().any(|c| c.source == path);
    s.pool = p.pool.iter().filter(|i| used(&i.path)).map(|i| PoolItem { id: Uuid::new_v4(), ..i.clone() }).collect();
    s.transcripts = p.transcripts.iter().filter(|t| used(&t.source)).cloned().collect();
    timeline::relayout(&mut s);
    Ok(s)
}

/// `<project> - <title>.forgevideo` beside the project file, numbered when that name is taken.
pub fn free_file(project_file: &Path, title: &str) -> PathBuf {
    let dir = project_file.parent().unwrap_or_else(|| Path::new("/"));
    let stem = project_file.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let safe: String = title.chars().map(|c| if c.is_alphanumeric() || " -_'!?,.()&".contains(c) { c } else { ' ' }).collect();
    let safe = safe.split_whitespace().collect::<Vec<_>>().join(" ");
    let safe: String = safe.trim_matches(|c| c == '.' || c == ' ').chars().take(60).collect();
    let base = format!("{stem} - {}", if safe.is_empty() { "Short" } else { safe.trim() });
    let mut file = dir.join(format!("{base}.forgevideo"));
    let mut n = 2;
    while file.exists() {
        file = dir.join(format!("{base} {n}.forgevideo"));
        n += 1;
    }
    file
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::captions::tests::{media, podcast};
    use crate::ai::captions::timeline_cues;
    use crate::project::Range;

    fn highlight(keep: &[(u64, u64)]) -> Highlight {
        Highlight {
            id: Uuid::new_v4(), title: "The hook".into(), reason: "r".into(),
            start: keep[0].0, end: keep[keep.len() - 1].1,
            keep: keep.iter().map(|&(start, end)| Range { start, end }).collect(),
            fade_in: 300, fade_out: 800, notes: vec![],
        }
    }

    #[test]
    fn short_keeps_only_the_ranges_and_spans_clip_boundaries() {
        let mut p = podcast();
        p.pool.push(PoolItem { id: Uuid::new_v4(), path: "/a.mp4".into(), media: media(60_000) });
        p.pool.push(PoolItem { id: Uuid::new_v4(), path: "/unused.mp4".into(), media: media(1_000) });
        p.clips[0].volume = 0.5;
        p.clips[0].transition_out = Transition::None;
        let before = p.clone();
        let s = build_short(&p, &highlight(&[(12_000, 20_000), (55_000, 65_000)])).unwrap();
        assert_eq!(p, before, "the podcast project is untouched");

        assert_eq!((s.name.as_str(), s.aspect), ("The hook", AspectPreset::Shorts9x16));
        let got: Vec<_> = s.clips.iter().map(|c| (c.source.to_str().unwrap(), c.source_start, c.source_end, c.timeline_start)).collect();
        assert_eq!(got, vec![("/a.mp4", 12_000, 20_000, 0), ("/a.mp4", 55_000, 60_000, 8_000), ("/b.mp4", 0, 5_000, 13_000)]);
        assert_eq!(s.duration_ms(), 18_000);
        assert_eq!((s.clips[0].fade_in, s.clips[0].fade_out), (300, 0));
        assert_eq!((s.clips[2].fade_in, s.clips[2].fade_out), (0, 800));
        assert_eq!(s.clips[0].volume, 0.5, "clip settings carry over");
        assert!(s.clips.iter().all(|c| p.clips.iter().all(|o| o.id != c.id)), "new ids");
        assert_eq!(s.pool.len(), 1, "only media the short uses");
        assert_eq!(s.transcripts.len(), 2);
        assert!(s.highlights.is_empty());
        let cues: Vec<_> = timeline_cues(&s).into_iter().map(|c| (c.start, c.end)).collect();
        assert_eq!(cues[0], (0, 7_000), "captions follow the cut: source 12-19 s");
        assert_eq!(*cues.last().unwrap(), (13_000, 18_000));
    }

    #[test]
    fn short_honours_trimmed_clips_and_rejects_stale_highlights() {
        let mut p = podcast();
        let a = p.clips[0].id;
        timeline::trim(&mut p, a, 20_000, 60_000).unwrap();
        let s = build_short(&p, &highlight(&[(5_000, 15_000)])).unwrap();
        assert_eq!((s.clips[0].source_start, s.clips[0].source_end), (25_000, 35_000), "timeline 5 s is source 25 s");
        assert!(matches!(build_short(&p, &highlight(&[(500_000, 520_000)])), Err(Error::InvalidEdit(_))));
        assert!(matches!(build_short(&Project::new("e"), &highlight(&[(0, 20_000)])), Err(Error::InvalidEdit(_))));
    }

    #[test]
    fn file_names_are_safe_and_never_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("Episode 12.forgevideo");
        let f = free_file(&project, "Why: \"sleep\" / matters?");
        assert_eq!(f, dir.path().join("Episode 12 - Why sleep matters?.forgevideo"));
        std::fs::write(&f, "x").unwrap();
        let f2 = free_file(&project, "Why: \"sleep\" / matters?");
        assert_eq!(f2, dir.path().join("Episode 12 - Why sleep matters? 2.forgevideo"));
        std::fs::write(&f2, "x").unwrap();
        assert!(free_file(&project, "Why: \"sleep\" / matters?").ends_with("Episode 12 - Why sleep matters? 3.forgevideo"));
        assert!(free_file(&project, "../..").ends_with("Episode 12 - Short.forgevideo"));
        assert!(free_file(&project, "").ends_with("Episode 12 - Short.forgevideo"));
        assert_eq!(free_file(&project, &"x".repeat(200)).file_stem().unwrap().len(), "Episode 12 - ".len() + 60);
    }

    #[test]
    fn relayout_drops_highlights_that_fell_off_the_timeline() {
        let mut p = podcast();
        let mut cut = highlight(&[(70_000, 80_000), (85_000, 90_000)]);
        cut.end = 90_000;
        p.highlights = vec![highlight(&[(10_000, 30_000)]), cut, highlight(&[(200_000, 220_000)])];
        timeline::relayout(&mut p);
        assert_eq!(p.highlights.len(), 2, "the one past the end is gone");
        let b = p.clips[1].id;
        timeline::trim(&mut p, b, 0, 22_000).unwrap();
        assert_eq!(p.duration_ms(), 82_000);
        let h = &p.highlights[1];
        assert_eq!((h.start, h.end), (70_000, 82_000));
        assert_eq!(h.keep, vec![Range { start: 70_000, end: 80_000 }], "the keep range past the end is dropped");
        timeline::delete(&mut p, b).unwrap();
        assert_eq!(p.highlights.len(), 1);
        let a = p.clips[0].id;
        timeline::delete(&mut p, a).unwrap();
        assert!(p.highlights.is_empty());
    }
}
