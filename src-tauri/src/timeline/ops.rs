//! Pure timeline operations. No I/O. Every op leaves the project in a valid, relaid-out state.
//! Invariant: clips are contiguous (ripple editing); a transition overlaps the following clip.

use crate::error::{Error, Result};
use crate::project::{Clip, Ms, Project, Transition};
use uuid::Uuid;

/// Minimum clip length we allow, so trims/splits can't produce zero-length clips.
pub const MIN_CLIP_MS: Ms = 100;

/// Recompute every `timeline_start` and clamp transitions so they never exceed the shorter neighbour.
pub fn relayout(p: &mut Project) {
    let n = p.clips.len();
    for i in 0..n {
        let next_len = if i + 1 < n { p.clips[i + 1].duration_ms() } else { 0 };
        let this_len = p.clips[i].duration_ms();
        let max_t = this_len.min(next_len) / 2;
        if i + 1 == n {
            p.clips[i].transition_out = Transition::None;
        } else {
            let t = &mut p.clips[i].transition_out;
            *t = match *t {
                Transition::None => Transition::None,
                Transition::CrossDissolve { ms } => Transition::CrossDissolve { ms: (ms as Ms).min(max_t) as u32 },
                Transition::DipToBlack { ms } => Transition::DipToBlack { ms: (ms as Ms).min(max_t) as u32 },
            };
        }
        let fi = p.clips[i].fade_in.min(this_len);
        p.clips[i].fade_in = fi;
        p.clips[i].fade_out = p.clips[i].fade_out.min(this_len - fi);
    }
    let mut cursor: Ms = 0;
    for i in 0..n {
        p.clips[i].timeline_start = cursor;
        cursor += p.clips[i].duration_ms();
        cursor = cursor.saturating_sub(p.clips[i].transition_out.duration_ms());
    }
}

fn idx(p: &Project, id: Uuid) -> Result<usize> {
    p.clip_index(id).ok_or(Error::ClipNotFound(id))
}

pub fn append(p: &mut Project, clip: Clip) {
    p.clips.push(clip);
    relayout(p);
}

/// Set the in/out points in *source* time.
pub fn trim(p: &mut Project, id: Uuid, source_start: Ms, source_end: Ms) -> Result<()> {
    let i = idx(p, id)?;
    let c = &mut p.clips[i];
    let end = source_end.min(c.media.duration_ms);
    if source_start + MIN_CLIP_MS > end {
        return Err(Error::InvalidEdit(format!("trim range too short ({source_start}..{end})")));
    }
    c.source_start = source_start;
    c.source_end = end;
    relayout(p);
    Ok(())
}

/// Split at a *timeline* position. The left half keeps the id; the right half is new.
/// Returns the new clip id.
pub fn split(p: &mut Project, id: Uuid, at_timeline_ms: Ms) -> Result<Uuid> {
    let i = idx(p, id)?;
    let c = &p.clips[i];
    if at_timeline_ms <= c.timeline_start || at_timeline_ms >= c.timeline_start + c.duration_ms() {
        return Err(Error::InvalidEdit("split point outside clip".into()));
    }
    let offset = at_timeline_ms - c.timeline_start;
    if offset < MIN_CLIP_MS || c.duration_ms() - offset < MIN_CLIP_MS {
        return Err(Error::InvalidEdit("split would create a clip that is too short".into()));
    }
    let cut = c.source_start + offset;
    let mut right = c.clone();
    right.id = Uuid::new_v4();
    right.source_start = cut;
    right.fade_in = 0;
    let left = &mut p.clips[i];
    left.source_end = cut;
    left.fade_out = 0;
    left.transition_out = Transition::None;
    let new_id = right.id;
    p.clips.insert(i + 1, right);
    relayout(p);
    Ok(new_id)
}

pub fn delete(p: &mut Project, id: Uuid) -> Result<()> {
    let i = idx(p, id)?;
    p.clips.remove(i);
    relayout(p);
    Ok(())
}

pub fn move_to(p: &mut Project, id: Uuid, to_index: usize) -> Result<()> {
    let i = idx(p, id)?;
    let c = p.clips.remove(i);
    let to = to_index.min(p.clips.len());
    p.clips.insert(to, c);
    relayout(p);
    Ok(())
}

pub fn set_fades(p: &mut Project, id: Uuid, fade_in: Ms, fade_out: Ms) -> Result<()> {
    let i = idx(p, id)?;
    p.clips[i].fade_in = fade_in;
    p.clips[i].fade_out = fade_out;
    relayout(p);
    Ok(())
}

pub fn set_transition(p: &mut Project, id: Uuid, t: Transition) -> Result<()> {
    let i = idx(p, id)?;
    p.clips[i].transition_out = t;
    relayout(p);
    Ok(())
}

pub fn set_volume(p: &mut Project, id: Uuid, volume: f32, muted: bool) -> Result<()> {
    let i = idx(p, id)?;
    p.clips[i].volume = volume.clamp(0.0, 2.0);
    p.clips[i].muted = muted;
    Ok(())
}

/// Which clip is under a timeline position, and the matching source time. Used by the preview.
pub fn locate(p: &Project, t: Ms) -> Option<(usize, Ms)> {
    p.clips.iter().enumerate().rev().find(|(_, c)| t >= c.timeline_start).map(|(i, c)| {
        let off = (t - c.timeline_start).min(c.duration_ms().saturating_sub(1));
        (i, c.source_start + off)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::*;
    use std::path::PathBuf;

    fn media(ms: Ms) -> MediaInfo {
        MediaInfo {
            duration_ms: ms, width: 1280, height: 720, fps: Rational { num: 30, den: 1 },
            codec: "h264".into(), container: "mov,mp4".into(), has_audio: true,
            audio_codec: Some("aac".into()), sample_rate: Some(48000), rotation: 0,
        }
    }
    fn proj(lens: &[Ms]) -> Project {
        let mut p = Project::new("t");
        for (i, l) in lens.iter().enumerate() {
            append(&mut p, Clip::new(PathBuf::from(format!("/{i}.mp4")), media(*l)));
        }
        p
    }

    #[test]
    fn append_is_contiguous() {
        let p = proj(&[5000, 4000, 3000]);
        let starts: Vec<_> = p.clips.iter().map(|c| c.timeline_start).collect();
        assert_eq!(starts, vec![0, 5000, 9000]);
        assert_eq!(p.duration_ms(), 12000);
    }

    #[test]
    fn trim_ripples_following_clips() {
        let mut p = proj(&[5000, 4000, 3000]);
        let id = p.clips[0].id;
        trim(&mut p, id, 1000, 3000).unwrap();
        assert_eq!(p.clips[0].duration_ms(), 2000);
        assert_eq!(p.clips[1].timeline_start, 2000);
        assert_eq!(p.clips[2].timeline_start, 6000);
        assert!(trim(&mut p, id, 1000, 1050).is_err());
        assert!(trim(&mut p, Uuid::new_v4(), 0, 1000).is_err());
    }

    #[test]
    fn split_keeps_source_continuity() {
        let mut p = proj(&[5000, 4000]);
        let id = p.clips[0].id;
        let new_id = split(&mut p, id, 2000).unwrap();
        assert_eq!(p.clips.len(), 3);
        assert_eq!(p.clips[0].id, id);
        assert_eq!(p.clips[1].id, new_id);
        assert_eq!(p.clips[0].source_end, 2000);
        assert_eq!(p.clips[1].source_start, 2000);
        assert_eq!(p.clips[1].source_end, 5000);
        assert_eq!(p.clips[1].timeline_start, 2000);
        assert_eq!(p.clips[2].timeline_start, 5000);
        assert!(split(&mut p, id, 0).is_err());
        assert!(split(&mut p, id, 6000).is_err());
    }

    #[test]
    fn delete_and_move_ripple() {
        let mut p = proj(&[5000, 4000, 3000]);
        let b = p.clips[1].id;
        delete(&mut p, b).unwrap();
        assert_eq!(p.clips.len(), 2);
        assert_eq!(p.clips[1].timeline_start, 5000);
        let c = p.clips[1].id;
        move_to(&mut p, c, 0).unwrap();
        assert_eq!(p.clips[0].id, c);
        assert_eq!(p.clips[0].timeline_start, 0);
        assert_eq!(p.clips[1].timeline_start, 3000);
    }

    #[test]
    fn transitions_overlap_and_are_clamped() {
        let mut p = proj(&[5000, 4000, 3000]);
        let a = p.clips[0].id;
        set_transition(&mut p, a, Transition::CrossDissolve { ms: 1000 }).unwrap();
        assert_eq!(p.clips[1].timeline_start, 4000);
        assert_eq!(p.clips[2].timeline_start, 8000);
        assert_eq!(p.duration_ms(), 11000);
        // clamp to half the shorter neighbour (3000/2 = 1500 for clip b->c)
        let b = p.clips[1].id;
        set_transition(&mut p, b, Transition::DipToBlack { ms: 9000 }).unwrap();
        assert_eq!(p.clips[1].transition_out, Transition::DipToBlack { ms: 1500 });
        // last clip can never have a transition out
        let c = p.clips[2].id;
        set_transition(&mut p, c, Transition::CrossDissolve { ms: 500 }).unwrap();
        assert_eq!(p.clips[2].transition_out, Transition::None);
    }

    #[test]
    fn locate_maps_timeline_to_source() {
        let mut p = proj(&[5000, 4000]);
        let a = p.clips[0].id;
        trim(&mut p, a, 1000, 3000).unwrap();
        assert_eq!(locate(&p, 500), Some((0, 1500)));
        assert_eq!(locate(&p, 2000), Some((1, 0)));
        assert_eq!(locate(&p, 2500), Some((1, 500)));
        assert_eq!(locate(&Project::new("e"), 0), None);
    }
}

#[cfg(test)]
mod edge_tests {
    use super::*;
    use crate::project::*;
    use std::path::PathBuf;

    fn media(ms: Ms) -> MediaInfo {
        MediaInfo {
            duration_ms: ms, width: 1280, height: 720, fps: Rational { num: 30, den: 1 },
            codec: "h264".into(), container: "mov,mp4".into(), has_audio: true,
            audio_codec: Some("aac".into()), sample_rate: Some(48000), rotation: 0,
        }
    }
    fn proj(lens: &[Ms]) -> Project {
        let mut p = Project::new("t");
        for (i, l) in lens.iter().enumerate() {
            append(&mut p, Clip::new(PathBuf::from(format!("/{i}.mp4")), media(*l)));
        }
        p
    }

    #[test]
    fn relayout_clamps_fades_to_clip_length() {
        let mut p = proj(&[1000]);
        let id = p.clips[0].id;
        set_fades(&mut p, id, 5000, 5000).unwrap();
        assert_eq!(p.clips[0].fade_in, 1000);
        assert_eq!(p.clips[0].fade_out, 0, "fade out gets whatever is left after fade in");
        set_fades(&mut p, id, 300, 900).unwrap();
        assert_eq!((p.clips[0].fade_in, p.clips[0].fade_out), (300, 700));
        assert!(set_fades(&mut p, Uuid::new_v4(), 0, 0).is_err());
    }

    #[test]
    fn relayout_on_empty_project_is_a_noop() {
        let mut p = Project::new("e");
        relayout(&mut p);
        assert_eq!(p.duration_ms(), 0);
    }

    #[test]
    fn trim_clamps_end_to_media_duration() {
        let mut p = proj(&[5000, 2000]);
        let id = p.clips[0].id;
        trim(&mut p, id, 4000, 99_000).unwrap();
        assert_eq!(p.clips[0].source_end, 5000);
        assert_eq!(p.clips[0].duration_ms(), 1000);
        assert_eq!(p.clips[1].timeline_start, 1000);
        // exactly MIN_CLIP_MS is allowed, one less is not
        trim(&mut p, id, 4900, 5000).unwrap();
        assert!(trim(&mut p, id, 4901, 5000).is_err());
    }

    #[test]
    fn trim_shrinks_transition_that_no_longer_fits() {
        let mut p = proj(&[5000, 5000]);
        let a = p.clips[0].id;
        set_transition(&mut p, a, Transition::CrossDissolve { ms: 2000 }).unwrap();
        trim(&mut p, a, 0, 1000).unwrap();
        assert_eq!(p.clips[0].transition_out, Transition::CrossDissolve { ms: 500 });
        assert_eq!(p.clips[1].timeline_start, 500);
    }

    #[test]
    fn split_respects_minimum_clip_length_on_both_sides() {
        let mut p = proj(&[1000]);
        let id = p.clips[0].id;
        assert!(split(&mut p, id, 50).is_err(), "left side too short");
        assert!(split(&mut p, id, 950).is_err(), "right side too short");
        assert!(split(&mut p, Uuid::new_v4(), 500).is_err());
        let new_id = split(&mut p, id, 100).unwrap();
        assert_eq!(p.clips[0].duration_ms(), 100);
        assert_eq!(p.clips[1].id, new_id);
        assert_eq!(p.clips[1].duration_ms(), 900);
        assert_eq!(p.duration_ms(), 1000, "splitting never changes total length");
    }

    #[test]
    fn split_moves_fades_and_transition_to_the_right_ends() {
        let mut p = proj(&[5000, 5000]);
        let a = p.clips[0].id;
        set_fades(&mut p, a, 500, 500).unwrap();
        set_transition(&mut p, a, Transition::DipToBlack { ms: 400 }).unwrap();
        p.clips[0].volume = 0.3;
        p.clips[0].muted = true;
        split(&mut p, a, 2500).unwrap();
        let (l, r) = (&p.clips[0], &p.clips[1]);
        assert_eq!((l.fade_in, l.fade_out), (500, 0));
        assert_eq!((r.fade_in, r.fade_out), (0, 500));
        assert_eq!(l.transition_out, Transition::None, "cut between halves is a hard cut");
        assert_eq!(r.transition_out, Transition::DipToBlack { ms: 400 });
        assert_eq!((r.volume, r.muted), (0.3, true), "audio settings are inherited");
        assert_eq!(r.source, l.source);
    }

    #[test]
    fn split_in_second_clip_uses_timeline_position() {
        let mut p = proj(&[5000, 4000]);
        let b = p.clips[1].id;
        let new_id = split(&mut p, b, 7000).unwrap();
        assert_eq!(p.clips[1].source_end, 2000);
        assert_eq!(p.clips[2].id, new_id);
        assert_eq!(p.clips[2].source_start, 2000);
        assert_eq!(p.clips[2].timeline_start, 7000);
    }

    #[test]
    fn delete_last_clip_leaves_empty_timeline() {
        let mut p = proj(&[5000]);
        let id = p.clips[0].id;
        delete(&mut p, id).unwrap();
        assert!(p.clips.is_empty());
        assert_eq!(p.duration_ms(), 0);
        assert!(delete(&mut p, id).is_err());
    }

    #[test]
    fn move_to_clamps_index_and_keeps_order_otherwise() {
        let mut p = proj(&[1000, 2000, 3000]);
        let ids: Vec<_> = p.clips.iter().map(|c| c.id).collect();
        move_to(&mut p, ids[0], 99).unwrap();
        assert_eq!(p.clips.iter().map(|c| c.id).collect::<Vec<_>>(), vec![ids[1], ids[2], ids[0]]);
        assert_eq!(p.clips[2].timeline_start, 5000);
        move_to(&mut p, ids[0], 2).unwrap();
        assert_eq!(p.clips[2].id, ids[0], "moving onto itself is stable");
        move_to(&mut p, ids[2], 0).unwrap();
        assert_eq!(p.clips.iter().map(|c| c.id).collect::<Vec<_>>(), vec![ids[2], ids[1], ids[0]]);
        assert!(move_to(&mut p, Uuid::new_v4(), 0).is_err());
    }

    #[test]
    fn moving_last_clip_away_restores_its_transition_slot() {
        let mut p = proj(&[3000, 3000]);
        let (a, b) = (p.clips[0].id, p.clips[1].id);
        set_transition(&mut p, a, Transition::CrossDissolve { ms: 500 }).unwrap();
        move_to(&mut p, b, 0).unwrap();
        // a is now last → its transition is dropped
        assert_eq!(p.clips[1].id, a);
        assert_eq!(p.clips[1].transition_out, Transition::None);
        assert_eq!(p.duration_ms(), 6000);
    }

    #[test]
    fn set_volume_clamps_and_does_not_relayout() {
        let mut p = proj(&[1000, 1000]);
        let id = p.clips[0].id;
        set_volume(&mut p, id, 9.0, false).unwrap();
        assert_eq!(p.clips[0].volume, 2.0);
        set_volume(&mut p, id, -1.0, true).unwrap();
        assert_eq!((p.clips[0].volume, p.clips[0].muted), (0.0, true));
        assert_eq!(p.clips[1].timeline_start, 1000);
        assert!(set_volume(&mut p, Uuid::new_v4(), 1.0, false).is_err());
    }

    #[test]
    fn set_transition_on_missing_clip_errors_and_clamps_odd_lengths() {
        let mut p = proj(&[1000, 3000]);
        let a = p.clips[0].id;
        assert!(set_transition(&mut p, Uuid::new_v4(), Transition::None).is_err());
        set_transition(&mut p, a, Transition::CrossDissolve { ms: 501 }).unwrap();
        assert_eq!(p.clips[0].transition_out, Transition::CrossDissolve { ms: 500 });
        set_transition(&mut p, a, Transition::None).unwrap();
        assert_eq!(p.clips[1].timeline_start, 1000);
    }

    #[test]
    fn locate_handles_ends_and_overlaps() {
        let mut p = proj(&[5000, 4000]);
        let a = p.clips[0].id;
        assert_eq!(locate(&p, 0), Some((0, 0)));
        assert_eq!(locate(&p, 4999), Some((0, 4999)));
        assert_eq!(locate(&p, 5000), Some((1, 0)));
        // past the end clamps into the last frame of the last clip
        assert_eq!(locate(&p, 50_000), Some((1, 3999)));
        // inside a dissolve the incoming clip wins
        set_transition(&mut p, a, Transition::CrossDissolve { ms: 1000 }).unwrap();
        assert_eq!(locate(&p, 4500), Some((1, 500)));
    }
}
