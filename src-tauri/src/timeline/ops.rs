//! Pure timeline operations. No I/O. Every op leaves the project in a valid, relaid-out state.
//! Invariants: V1 clips are contiguous (ripple editing) and a transition overlaps the following
//! clip; overlay and audio clips are free-positioned, sorted, and never overlap within a track.

use crate::error::{Error, Result};
use crate::project::{AudioClip, Clip, Ms, OverlayClip, Placement, Project, Transition};
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

    // Overlay track: clamp each clip to its source, then sort and push apart so nothing overlaps.
    for o in &mut p.overlays {
        if !o.media.is_still {
            o.source_end = o.source_end.min(o.media.duration_ms);
        }
        if o.source_end < o.source_start + MIN_CLIP_MS {
            o.source_end = o.source_start + MIN_CLIP_MS;
        }
        let len = o.duration_ms();
        o.fade_in = o.fade_in.min(len);
        o.fade_out = o.fade_out.min(len - o.fade_in);
        o.placement = o.placement.clamped();
    }
    p.overlays.sort_by_key(|o| (o.layer, o.timeline_start));
    let mut end: Ms = 0;
    let mut layer = u32::MAX;
    for o in &mut p.overlays {
        if o.layer != layer { layer = o.layer; end = 0; }
        o.timeline_start = o.timeline_start.max(end);
        end = o.end_ms();
    }
    let needed = p.overlays.iter().map(|o| o.layer + 1).max().unwrap_or(1);
    p.overlay_layers = p.overlay_layers.max(needed).max(1);

    p.video_volume = if p.video_volume.is_finite() { p.video_volume.clamp(0.0, 1.0) } else { 1.0 };
    // Audio tracks: same rules per track.
    for t in &mut p.audio_tracks {
        t.volume = if t.volume.is_finite() { t.volume.clamp(0.0, 1.0) } else { 1.0 };
        for c in &mut t.clips {
            c.source_end = c.source_end.min(c.media.duration_ms);
            if c.source_end < c.source_start + MIN_CLIP_MS {
                c.source_start = c.source_end.saturating_sub(MIN_CLIP_MS);
            }
            let len = c.duration_ms();
            c.fade_in = c.fade_in.min(len);
            c.fade_out = c.fade_out.min(len - c.fade_in);
            c.volume = c.volume.clamp(0.0, 2.0);
        }
        t.clips.sort_by_key(|c| c.timeline_start);
        let mut end: Ms = 0;
        for c in &mut t.clips {
            c.timeline_start = c.timeline_start.max(end);
            end = c.end_ms();
        }
    }

    // Highlights: suggestions that no longer fit on the timeline are dropped.
    let total = p.duration_ms();
    for h in &mut p.highlights {
        h.end = h.end.min(total);
        let (s, e) = (h.start, h.end);
        for r in &mut h.keep {
            r.start = r.start.clamp(s, e.max(s));
            r.end = r.end.clamp(s, e.max(s));
        }
        h.keep.retain(|r| r.start + MIN_CLIP_MS <= r.end);
    }
    p.highlights.retain(|h| h.start + MIN_CLIP_MS <= h.end && !h.keep.is_empty());
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
    // Stills have no source length: the range is simply how long the image stays up.
    let end = if c.media.is_still { source_end } else { source_end.min(c.media.duration_ms) };
    check_range(source_start, end)?;
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
    let cut = split_point(c.timeline_start, c.source_start, c.duration_ms(), at_timeline_ms)?;
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

/// Sorted, de-duplicated positions of `ids` within a row, which must sit side by side.
fn run_of(ids: &[Uuid], find: impl Fn(Uuid) -> Option<usize>) -> Result<Vec<usize>> {
    if ids.len() < 2 {
        return Err(Error::InvalidEdit("select two or more neighbouring clips to merge".into()));
    }
    let mut ix = ids.iter().map(|id| find(*id).ok_or(Error::ClipNotFound(*id))).collect::<Result<Vec<_>>>()?;
    ix.sort_unstable();
    ix.dedup();
    if ix.len() < 2 || ix.windows(2).any(|w| w[1] != w[0] + 1) {
        return Err(Error::InvalidEdit("only neighbouring clips can be merged".into()));
    }
    Ok(ix)
}

/// Two pieces join when they come from the same file and the right one continues where the left
/// stopped (stills have no source time, so only the file has to match).
fn joinable(a_source: &std::path::Path, a_source_end: Ms, still: bool, b_source: &std::path::Path, b_source_start: Ms) -> Result<()> {
    if a_source != b_source {
        return Err(Error::InvalidEdit("only pieces of the same file can be merged".into()));
    }
    if !still && a_source_end != b_source_start {
        return Err(Error::InvalidEdit("these clips are not consecutive pieces of the file".into()));
    }
    Ok(())
}

/// Undo a split: join neighbouring pieces of one file back into a single clip. The first piece
/// keeps its id, name and fade-in; the last one supplies the fade-out (and, on V1, the transition).
/// Works on V1, one overlay layer or one audio track; free clips must also touch on the timeline.
pub fn merge(p: &mut Project, ids: &[Uuid]) -> Result<Uuid> {
    if ids.iter().all(|id| p.clip_index(*id).is_some()) {
        let ix = run_of(ids, |id| p.clip_index(id))?;
        for w in ix.windows(2) {
            let (a, b) = (&p.clips[w[0]], &p.clips[w[1]]);
            joinable(&a.source, a.source_end, a.media.is_still, &b.source, b.source_start)?;
        }
        let total: Ms = ix.iter().map(|i| p.clips[*i].duration_ms()).sum();
        let last = p.clips[*ix.last().unwrap()].clone();
        let first = &mut p.clips[ix[0]];
        first.source_end = first.source_start + total;
        first.fade_out = last.fade_out;
        first.transition_out = last.transition_out;
        let id = first.id;
        for i in ix[1..].iter().rev() {
            p.clips.remove(*i);
        }
        relayout(p);
        return Ok(id);
    }
    if ids.iter().all(|id| p.overlay_index(*id).is_some()) {
        let layer = p.overlays[p.overlay_index(ids[0]).unwrap()].layer;
        if ids.iter().any(|id| p.overlays[p.overlay_index(*id).unwrap()].layer != layer) {
            return Err(Error::InvalidEdit("only neighbouring clips can be merged".into()));
        }
        let mut row: Vec<usize> = (0..p.overlays.len()).filter(|i| p.overlays[*i].layer == layer).collect();
        row.sort_by_key(|i| p.overlays[*i].timeline_start);
        let ix = run_of(ids, |id| row.iter().position(|i| p.overlays[*i].id == id))?;
        let ix: Vec<usize> = ix.into_iter().map(|k| row[k]).collect();
        for w in ix.windows(2) {
            let (a, b) = (&p.overlays[w[0]], &p.overlays[w[1]]);
            joinable(&a.source, a.source_end, a.media.is_still, &b.source, b.source_start)?;
            if a.timeline_start + a.duration_ms() != b.timeline_start {
                return Err(Error::InvalidEdit("these clips do not touch on the timeline".into()));
            }
        }
        let total: Ms = ix.iter().map(|i| p.overlays[*i].duration_ms()).sum();
        let last = p.overlays[*ix.last().unwrap()].clone();
        let first = &mut p.overlays[ix[0]];
        first.source_end = first.source_start + total;
        first.fade_out = last.fade_out;
        let id = first.id;
        let mut drop: Vec<usize> = ix[1..].to_vec();
        drop.sort_unstable();
        for i in drop.iter().rev() {
            p.overlays.remove(*i);
        }
        relayout(p);
        return Ok(id);
    }
    let (ti, _) = aidx(p, ids[0])?;
    let ix = run_of(ids, |id| p.audio_tracks[ti].clips.iter().position(|c| c.id == id))?;
    let clips = &p.audio_tracks[ti].clips;
    for w in ix.windows(2) {
        let (a, b) = (&clips[w[0]], &clips[w[1]]);
        joinable(&a.source, a.source_end, false, &b.source, b.source_start)?;
        if a.end_ms() != b.timeline_start {
            return Err(Error::InvalidEdit("these clips do not touch on the timeline".into()));
        }
    }
    let total: Ms = ix.iter().map(|i| clips[*i].duration_ms()).sum();
    let last = clips[*ix.last().unwrap()].clone();
    let first = &mut p.audio_tracks[ti].clips[ix[0]];
    first.source_end = first.source_start + total;
    first.fade_out = last.fade_out;
    let id = first.id;
    for i in ix[1..].iter().rev() {
        p.audio_tracks[ti].clips.remove(*i);
    }
    relayout(p);
    Ok(id)
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

/// Longest clip name kept, in characters.
pub const NAME_MAX: usize = 80;

/// Rename a clip on any track (V1, overlay or audio). A blank name restores the file name.
pub fn rename(p: &mut Project, id: Uuid, name: &str) -> Result<()> {
    let name = name.trim();
    let name = (!name.is_empty()).then(|| name.chars().take(NAME_MAX).collect::<String>());
    if let Some(c) = p.clips.iter_mut().find(|c| c.id == id) {
        c.name = name;
    } else if let Some(o) = p.overlays.iter_mut().find(|o| o.id == id) {
        o.name = name;
    } else {
        let (t, c) = aidx(p, id)?;
        p.audio_tracks[t].clips[c].name = name;
    }
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

/// Shared by every split: returns the source cut point given a timeline split inside a clip.
fn split_point(timeline_start: Ms, source_start: Ms, len: Ms, at_timeline_ms: Ms) -> Result<Ms> {
    if at_timeline_ms <= timeline_start || at_timeline_ms >= timeline_start + len {
        return Err(Error::InvalidEdit("split point outside clip".into()));
    }
    let offset = at_timeline_ms - timeline_start;
    if offset < MIN_CLIP_MS || len - offset < MIN_CLIP_MS {
        return Err(Error::InvalidEdit("split would create a clip that is too short".into()));
    }
    Ok(source_start + offset)
}

fn check_range(source_start: Ms, source_end: Ms) -> Result<()> {
    if source_start + MIN_CLIP_MS > source_end {
        return Err(Error::InvalidEdit(format!("trim range too short ({source_start}..{source_end})")));
    }
    Ok(())
}

// ---------- Overlay track (V2) ----------

fn oidx(p: &Project, id: Uuid) -> Result<usize> {
    p.overlay_index(id).ok_or(Error::ClipNotFound(id))
}

/// A clip that was just placed or moved wins its spot: anything it lands on is pushed after it
/// (relayout then chains further pushes).
fn claim(starts: &mut [(Ms, Ms)], winner: usize) {
    let (ws, we) = starts[winner];
    for (i, (s, e)) in starts.iter_mut().enumerate() {
        if i != winner && *s < we && ws < *e {
            let len = *e - *s;
            *s = we;
            *e = we + len;
        }
    }
}

fn claim_overlay(p: &mut Project, winner: usize) {
    let layer = p.overlays[winner].layer;
    // Clips on other layers never collide: give them a span that overlaps nothing.
    let mut spans: Vec<(Ms, Ms)> = p.overlays.iter().map(|o| if o.layer == layer { (o.timeline_start, o.end_ms()) } else { (Ms::MAX, Ms::MAX) }).collect();
    claim(&mut spans, winner);
    for (o, (s, _)) in p.overlays.iter_mut().zip(spans) { if o.layer == layer { o.timeline_start = s; } }
}

fn claim_audio(t: &mut crate::project::AudioTrack, winner: usize) {
    let mut spans: Vec<(Ms, Ms)> = t.clips.iter().map(|c| (c.timeline_start, c.end_ms())).collect();
    claim(&mut spans, winner);
    for (c, (s, _)) in t.clips.iter_mut().zip(spans) { c.timeline_start = s; }
}

pub fn overlay_add(p: &mut Project, mut clip: OverlayClip, at: Ms, layer: u32) -> Uuid {
    clip.timeline_start = at;
    clip.layer = layer;
    let id = clip.id;
    p.overlays.push(clip);
    let i = p.overlays.len() - 1;
    claim_overlay(p, i);
    relayout(p);
    id
}

pub fn overlay_move(p: &mut Project, id: Uuid, timeline_start: Ms, layer: u32) -> Result<()> {
    let i = oidx(p, id)?;
    p.overlays[i].timeline_start = timeline_start;
    p.overlays[i].layer = layer;
    claim_overlay(p, i);
    relayout(p);
    Ok(())
}

pub fn overlay_layer_add(p: &mut Project) {
    p.overlay_layers += 1;
}

/// Remove an overlay row and everything on it; higher rows shift down. The last row cannot go.
pub fn overlay_layer_remove(p: &mut Project, layer: u32) -> Result<()> {
    if p.overlay_layers <= 1 || layer >= p.overlay_layers {
        return Err(Error::InvalidEdit("cannot remove that overlay layer".into()));
    }
    p.overlays.retain(|o| o.layer != layer);
    for o in &mut p.overlays { if o.layer > layer { o.layer -= 1; } }
    p.overlay_layers -= 1;
    relayout(p);
    Ok(())
}

pub fn overlay_trim(p: &mut Project, id: Uuid, source_start: Ms, source_end: Ms) -> Result<()> {
    let i = oidx(p, id)?;
    let o = &mut p.overlays[i];
    let (start, end) = if o.media.is_still { (0, source_end) } else { (source_start, source_end.min(o.media.duration_ms)) };
    check_range(start, end)?;
    o.source_start = start;
    o.source_end = end;
    relayout(p);
    Ok(())
}

pub fn overlay_split(p: &mut Project, id: Uuid, at_timeline_ms: Ms) -> Result<Uuid> {
    let i = oidx(p, id)?;
    let o = &p.overlays[i];
    let cut = split_point(o.timeline_start, o.source_start, o.duration_ms(), at_timeline_ms)?;
    let mut right = o.clone();
    right.id = Uuid::new_v4();
    right.fade_in = 0;
    right.timeline_start = at_timeline_ms;
    if o.media.is_still {
        // Stills have no source time: the right half is simply the remaining length.
        right.source_start = 0;
        right.source_end = o.source_end - cut;
    } else {
        right.source_start = cut;
    }
    let left = &mut p.overlays[i];
    left.source_end = cut;
    left.fade_out = 0;
    let new_id = right.id;
    p.overlays.insert(i + 1, right);
    relayout(p);
    Ok(new_id)
}

pub fn overlay_delete(p: &mut Project, id: Uuid) -> Result<()> {
    let i = oidx(p, id)?;
    p.overlays.remove(i);
    relayout(p);
    Ok(())
}

pub fn overlay_set_fades(p: &mut Project, id: Uuid, fade_in: Ms, fade_out: Ms) -> Result<()> {
    let i = oidx(p, id)?;
    p.overlays[i].fade_in = fade_in;
    p.overlays[i].fade_out = fade_out;
    relayout(p);
    Ok(())
}

pub fn overlay_set_placement(p: &mut Project, id: Uuid, placement: Placement) -> Result<()> {
    let i = oidx(p, id)?;
    p.overlays[i].placement = placement.clamped();
    Ok(())
}

/// Overlays under a timeline position, lowest layer first, with matching source times.
pub fn locate_overlays(p: &Project, t: Ms) -> Vec<(usize, Ms)> {
    p.overlays.iter().enumerate().filter(|(_, o)| t >= o.timeline_start && t < o.end_ms()).map(|(i, o)| (i, o.source_start + (t - o.timeline_start))).collect()
}

// ---------- Audio tracks ----------

fn tidx(p: &Project, id: Uuid) -> Result<usize> {
    p.audio_track_index(id).ok_or(Error::ClipNotFound(id))
}

fn aidx(p: &Project, id: Uuid) -> Result<(usize, usize)> {
    p.audio_clip_index(id).ok_or(Error::ClipNotFound(id))
}

pub fn audio_track_add(p: &mut Project, label: &str) -> Uuid {
    let t = crate::project::AudioTrack::new(label.trim().is_empty().then(|| "Audio").unwrap_or(label.trim()));
    let id = t.id;
    p.audio_tracks.push(t);
    id
}

pub fn audio_track_update(p: &mut Project, id: Uuid, label: &str, muted: bool, volume: f32) -> Result<()> {
    let i = tidx(p, id)?;
    if !label.trim().is_empty() {
        p.audio_tracks[i].label = label.trim().to_string();
    }
    p.audio_tracks[i].muted = muted;
    p.audio_tracks[i].volume = if volume.is_finite() { volume.clamp(0.0, 1.0) } else { 1.0 };
    Ok(())
}

pub fn audio_track_remove(p: &mut Project, id: Uuid) -> Result<()> {
    let i = tidx(p, id)?;
    p.audio_tracks.remove(i);
    Ok(())
}

pub fn audio_clip_add(p: &mut Project, track_id: Uuid, mut clip: AudioClip, at: Ms) -> Result<Uuid> {
    let ti = tidx(p, track_id)?;
    clip.timeline_start = at;
    let id = clip.id;
    p.audio_tracks[ti].clips.push(clip);
    let ci = p.audio_tracks[ti].clips.len() - 1;
    claim_audio(&mut p.audio_tracks[ti], ci);
    relayout(p);
    Ok(id)
}

/// "Split audio from video": copy the V1 clip's audio onto an audio track as a free clip with the
/// same range, volume and fades, and mute the video clip. Uses the first track, adding one if needed.
pub fn detach_audio(p: &mut Project, id: Uuid, track_id: Option<Uuid>) -> Result<Uuid> {
    let i = idx(p, id)?;
    let c = &p.clips[i];
    if !c.media.has_audio || c.media.is_still {
        return Err(Error::InvalidEdit("clip has no audio to split off".into()));
    }
    let mut a = AudioClip::new(c.source.clone(), c.media.clone());
    a.name = c.name.clone();
    a.source_start = c.source_start;
    a.source_end = c.source_end;
    a.volume = c.volume;
    a.fade_in = c.fade_in;
    a.fade_out = c.fade_out;
    let at = c.timeline_start;
    let track = match track_id.or_else(|| p.audio_tracks.first().map(|t| t.id)) {
        Some(t) => t,
        None => audio_track_add(p, "Audio"),
    };
    let new_id = audio_clip_add(p, track, a, at)?;
    p.clips[i].muted = true;
    Ok(new_id)
}

/// Move within or between tracks.
pub fn audio_clip_move(p: &mut Project, id: Uuid, track_id: Uuid, timeline_start: Ms) -> Result<()> {
    let (ti, ci) = aidx(p, id)?;
    let to = tidx(p, track_id)?;
    let mut c = p.audio_tracks[ti].clips.remove(ci);
    c.timeline_start = timeline_start;
    p.audio_tracks[to].clips.push(c);
    let ci = p.audio_tracks[to].clips.len() - 1;
    claim_audio(&mut p.audio_tracks[to], ci);
    relayout(p);
    Ok(())
}

pub fn audio_clip_trim(p: &mut Project, id: Uuid, source_start: Ms, source_end: Ms) -> Result<()> {
    let (ti, ci) = aidx(p, id)?;
    let c = &mut p.audio_tracks[ti].clips[ci];
    let end = source_end.min(c.media.duration_ms);
    check_range(source_start, end)?;
    c.source_start = source_start;
    c.source_end = end;
    relayout(p);
    Ok(())
}

pub fn audio_clip_split(p: &mut Project, id: Uuid, at_timeline_ms: Ms) -> Result<Uuid> {
    let (ti, ci) = aidx(p, id)?;
    let c = &p.audio_tracks[ti].clips[ci];
    let cut = split_point(c.timeline_start, c.source_start, c.duration_ms(), at_timeline_ms)?;
    let mut right = c.clone();
    right.id = Uuid::new_v4();
    right.source_start = cut;
    right.fade_in = 0;
    right.timeline_start = at_timeline_ms;
    let left = &mut p.audio_tracks[ti].clips[ci];
    left.source_end = cut;
    left.fade_out = 0;
    let new_id = right.id;
    p.audio_tracks[ti].clips.insert(ci + 1, right);
    relayout(p);
    Ok(new_id)
}

pub fn audio_clip_delete(p: &mut Project, id: Uuid) -> Result<()> {
    let (ti, ci) = aidx(p, id)?;
    p.audio_tracks[ti].clips.remove(ci);
    relayout(p);
    Ok(())
}

pub fn audio_clip_set(p: &mut Project, id: Uuid, volume: f32, fade_in: Ms, fade_out: Ms, muted: bool) -> Result<()> {
    let (ti, ci) = aidx(p, id)?;
    let c = &mut p.audio_tracks[ti].clips[ci];
    c.volume = volume.clamp(0.0, 2.0);
    c.fade_in = fade_in;
    c.fade_out = fade_out;
    c.muted = muted;
    relayout(p);
    Ok(())
}

#[cfg(test)]
mod track_tests {
    use super::*;
    use crate::project::*;
    use std::path::PathBuf;

    fn vid(ms: Ms) -> MediaInfo {
        MediaInfo {
            duration_ms: ms, width: 1280, height: 720, fps: Rational { num: 30, den: 1 },
            codec: "h264".into(), container: "mov,mp4".into(), has_audio: true,
            audio_codec: Some("aac".into()), sample_rate: Some(48000), rotation: 0, is_still: false,
        }
    }
    fn aud(ms: Ms) -> MediaInfo { let mut m = vid(ms); m.width = 0; m.height = 0; m }
    fn png() -> MediaInfo { let mut m = vid(STILL_DEFAULT_MS); m.is_still = true; m.has_audio = false; m }

    #[test]
    fn merge_joins_split_overlays_and_audio_clips_that_touch() {
        let mut p = Project::new("m");
        append(&mut p, Clip::new(PathBuf::from("/v.mp4"), vid(20_000)));
        // a still overlay: halves have no source continuity, only length
        let logo = overlay_add(&mut p, OverlayClip::new(PathBuf::from("/logo.png"), png()), 1000, 0);
        overlay_set_fades(&mut p, logo, 200, 300).unwrap();
        let before = p.clone();
        let right = overlay_split(&mut p, logo, 3000).unwrap();
        assert_eq!(merge(&mut p, &[right, logo]).unwrap(), logo);
        assert_eq!(p, before);
        // a video overlay whose halves drifted apart no longer merges
        let b = overlay_add(&mut p, OverlayClip::new(PathBuf::from("/b.mp4"), vid(8000)), 7000, 0);
        let br = overlay_split(&mut p, b, 9000).unwrap();
        overlay_move(&mut p, br, 12_000, 0).unwrap();
        assert!(matches!(merge(&mut p, &[b, br]), Err(Error::InvalidEdit(m)) if m.contains("touch")));
        // pieces on different layers are not neighbours
        let c = overlay_add(&mut p, OverlayClip::new(PathBuf::from("/b.mp4"), vid(8000)), 0, 1);
        assert!(matches!(merge(&mut p, &[b, c]), Err(Error::InvalidEdit(m)) if m.contains("neighbouring")));

        let t = audio_track_add(&mut p, "Music");
        let m = audio_clip_add(&mut p, t, AudioClip::new("/m.m4a".into(), aud(9000)), 500).unwrap();
        audio_clip_set(&mut p, m, 0.5, 100, 200, false).unwrap();
        let before = p.clone();
        let m2 = audio_clip_split(&mut p, m, 3000).unwrap();
        let m3 = audio_clip_split(&mut p, m2, 6000).unwrap();
        assert_eq!(merge(&mut p, &[m3, m, m2]).unwrap(), m);
        assert_eq!(p, before);
        let m2 = audio_clip_split(&mut p, m, 3000).unwrap();
        audio_clip_move(&mut p, m2, t, 5000).unwrap();
        assert!(matches!(merge(&mut p, &[m, m2]), Err(Error::InvalidEdit(m)) if m.contains("touch")));
    }

    #[test]
    fn overlays_sort_and_push_apart() {
        let mut p = Project::new("o");
        let a = overlay_add(&mut p, OverlayClip::new("/a.mp4".into(), vid(4000)), 3000, 0);
        let b = overlay_add(&mut p, OverlayClip::new("/b.png".into(), png()), 0, 0);
        assert_eq!(p.overlays[0].id, b);
        assert_eq!(p.overlays[1].id, a);
        // b (5 s) now overlaps a at 3000 → a is pushed to 5000
        assert_eq!(p.overlays[1].timeline_start, 5000);
        overlay_move(&mut p, a, 1000, 0).unwrap();
        // the moved clip wins; b is pushed after it
        assert_eq!(p.overlays.iter().find(|o| o.id == a).unwrap().timeline_start, 1000);
        assert_eq!(p.overlays.iter().find(|o| o.id == b).unwrap().timeline_start, 5000);
        assert_eq!(locate_overlays(&p, 2000), vec![(0, 1000)]);
        assert!(locate_overlays(&p, 10_000).is_empty());
        // a second layer: clips there may overlap the first layer freely
        assert_eq!(p.overlay_layers, 1);
        overlay_layer_add(&mut p);
        assert_eq!(p.overlay_layers, 2);
        let c = overlay_add(&mut p, OverlayClip::new("/c.mp4".into(), vid(4000)), 1500, 1);
        let co = p.overlays.iter().find(|o| o.id == c).unwrap();
        assert_eq!((co.layer, co.timeline_start), (1, 1500));
        assert_eq!(p.overlays.iter().find(|o| o.id == a).unwrap().timeline_start, 1000, "layer 0 untouched");
        assert_eq!(locate_overlays(&p, 2000).len(), 2);
        // moving between layers, and removing a layer drops its clips and renumbers
        overlay_move(&mut p, b, 0, 1).unwrap();
        assert_eq!(p.overlays.iter().find(|o| o.id == b).unwrap().layer, 1);
        assert_eq!(p.overlays.iter().find(|o| o.id == c).unwrap().timeline_start, 5000, "pushed by b on layer 1");
        overlay_add(&mut p, OverlayClip::new("/d.mp4".into(), vid(1000)), 0, 5);
        assert_eq!(p.overlay_layers, 6, "layers grow to fit");
        assert!(overlay_layer_remove(&mut p, 9).is_err());
        overlay_layer_remove(&mut p, 1).unwrap();
        assert_eq!(p.overlays.len(), 2);
        assert_eq!(p.overlays.iter().find(|o| o.source.ends_with("d.mp4")).unwrap().layer, 4);
        assert_eq!(p.overlay_layers, 5);
        for _ in 0..4 { overlay_layer_remove(&mut p, 1).unwrap(); }
        assert!(overlay_layer_remove(&mut p, 0).is_err(), "last layer stays");
    }

    #[test]
    fn overlay_trim_split_and_fades() {
        let mut p = Project::new("o");
        let a = overlay_add(&mut p, OverlayClip::new("/a.mp4".into(), vid(4000)), 0, 0);
        overlay_trim(&mut p, a, 1000, 99_000).unwrap();
        assert_eq!((p.overlays[0].source_start, p.overlays[0].source_end), (1000, 4000));
        assert!(overlay_trim(&mut p, a, 3950, 4000).is_err());
        overlay_set_fades(&mut p, a, 10_000, 10_000).unwrap();
        assert_eq!((p.overlays[0].fade_in, p.overlays[0].fade_out), (3000, 0));
        let r = overlay_split(&mut p, a, 1500).unwrap();
        assert_eq!(p.overlays[0].source_end, 2500);
        assert_eq!((p.overlays[1].id, p.overlays[1].source_start, p.overlays[1].timeline_start), (r, 2500, 1500));
        assert!(overlay_split(&mut p, a, 0).is_err());
        // stills: trim sets a length, split keeps lengths
        let s = overlay_add(&mut p, OverlayClip::new("/l.png".into(), png()), 10_000, 0);
        overlay_trim(&mut p, s, 500, 8000).unwrap();
        let so = p.overlays.iter().find(|o| o.id == s).unwrap();
        assert_eq!((so.source_start, so.source_end), (0, 8000), "stills always start at 0");
        let r2 = overlay_split(&mut p, s, 13_000).unwrap();
        let (l, r) = (p.overlays.iter().find(|o| o.id == s).unwrap(), p.overlays.iter().find(|o| o.id == r2).unwrap());
        assert_eq!((l.duration_ms(), r.duration_ms(), r.timeline_start), (3000, 5000, 13_000));
        overlay_delete(&mut p, s).unwrap();
        assert!(overlay_delete(&mut p, s).is_err());
        overlay_set_placement(&mut p, a, Placement { scale: 5.0, x: 0.1, y: 0.2 }).unwrap();
        assert_eq!(p.overlays[0].placement, Placement { scale: 1.0, x: 0.1, y: 0.2 });
    }

    #[test]
    fn stills_on_v1_stretch_freely() {
        let mut p = Project::new("s");
        append(&mut p, Clip::new("/l.png".into(), png()));
        let id = p.clips[0].id;
        assert_eq!(p.duration_ms(), STILL_DEFAULT_MS);
        trim(&mut p, id, 0, 12_000).unwrap();
        assert_eq!(p.duration_ms(), 12_000);
        assert!(trim(&mut p, id, 0, 50).is_err());
        let r = split(&mut p, id, 4000).unwrap();
        assert_eq!((p.clips[0].source_end, p.clips[1].id, p.clips[1].source_start), (4000, r, 4000));
    }

    #[test]
    fn audio_tracks_and_clips() {
        let mut p = Project::new("a");
        let music = audio_track_add(&mut p, "Music");
        let sfx = audio_track_add(&mut p, "   ");
        assert_eq!(p.audio_tracks[1].label, "Audio");
        audio_track_update(&mut p, sfx, "SFX", true, 1.0).unwrap();
        assert_eq!((p.audio_tracks[1].label.as_str(), p.audio_tracks[1].muted), ("SFX", true));
        audio_track_update(&mut p, sfx, "SFX", true, 3.0).unwrap();
        assert_eq!(p.audio_tracks[1].volume, 1.0, "fader tops out at 1");
        audio_track_update(&mut p, sfx, "SFX", true, 0.3).unwrap();
        assert_eq!(p.audio_tracks[1].volume, 0.3);
        p.audio_tracks[1].volume = f32::NAN;
        p.video_volume = -2.0;
        relayout(&mut p);
        assert_eq!((p.audio_tracks[1].volume, p.video_volume), (1.0, 0.0), "relayout repairs faders");
        let a = audio_clip_add(&mut p, music, AudioClip::new("/m.m4a".into(), aud(9000)), 2000).unwrap();
        let b = audio_clip_add(&mut p, music, AudioClip::new("/n.m4a".into(), aud(1000)), 2500).unwrap();
        // the dropped clip wins its spot; the 9 s clip it landed on is pushed after it
        assert_eq!(p.audio_tracks[0].clips[0].id, b);
        assert_eq!(p.audio_tracks[0].clips[1].id, a);
        assert_eq!(p.audio_tracks[0].clips[1].timeline_start, 3500);
        audio_clip_move(&mut p, b, sfx, 0).unwrap();
        assert_eq!(p.audio_tracks[0].clips.len(), 1);
        assert_eq!(p.audio_tracks[1].clips[0].id, b);
        assert!(audio_clip_move(&mut p, b, Uuid::new_v4(), 0).is_err());
        audio_clip_move(&mut p, a, music, 2000).unwrap();
        audio_clip_trim(&mut p, a, 1000, 99_000).unwrap();
        assert_eq!(p.audio_tracks[0].clips[0].source_end, 9000);
        audio_clip_set(&mut p, a, 5.0, 100_000, 5, true).unwrap();
        let c = &p.audio_tracks[0].clips[0];
        assert_eq!((c.volume, c.fade_in, c.fade_out, c.muted), (2.0, 8000, 0, true));
        let r = audio_clip_split(&mut p, a, 6000).unwrap();
        assert_eq!(p.audio_tracks[0].clips[0].source_end, 5000);
        assert_eq!((p.audio_tracks[0].clips[1].id, p.audio_tracks[0].clips[1].timeline_start), (r, 6000));
        audio_clip_delete(&mut p, r).unwrap();
        assert!(audio_clip_delete(&mut p, r).is_err());
        audio_track_remove(&mut p, music).unwrap();
        assert!(audio_track_remove(&mut p, music).is_err());
        assert_eq!(p.audio_tracks.len(), 1);
        assert_eq!(p.duration_ms(), 0, "audio never defines the project length");
    }
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
            audio_codec: Some("aac".into()), sample_rate: Some(48000), rotation: 0, is_still: false,
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
            audio_codec: Some("aac".into()), sample_rate: Some(48000), rotation: 0, is_still: false,
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
    fn rename_reaches_every_track_and_blank_restores_the_file_name() {
        let mut p = proj(&[2000]);
        let v = p.clips[0].id;
        let o = overlay_add(&mut p, OverlayClip::new("/o.mp4".into(), media(1000)), 0, 0);
        let t = audio_track_add(&mut p, "Music");
        let a = audio_clip_add(&mut p, t, AudioClip::new("/a.m4a".into(), media(1000)), 0).unwrap();
        rename(&mut p, v, "  Intro  ").unwrap();
        rename(&mut p, o, "Logo").unwrap();
        rename(&mut p, a, &"x".repeat(200)).unwrap();
        assert_eq!(p.clips[0].name.as_deref(), Some("Intro"));
        assert_eq!(p.overlays[0].name.as_deref(), Some("Logo"));
        assert_eq!(p.audio_tracks[0].clips[0].name.as_ref().unwrap().chars().count(), NAME_MAX);
        // both halves of a split keep the name, and so does detached audio
        let right = split(&mut p, v, 1000).unwrap();
        assert_eq!(p.clips.iter().find(|c| c.id == right).unwrap().name.as_deref(), Some("Intro"));
        let d = detach_audio(&mut p, v, Some(t)).unwrap();
        assert_eq!(p.audio_tracks[0].clips.iter().find(|c| c.id == d).unwrap().name.as_deref(), Some("Intro"));
        rename(&mut p, v, "   ").unwrap();
        assert_eq!(p.clips[0].name, None);
        assert!(rename(&mut p, Uuid::new_v4(), "x").is_err());
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
    fn merge_undoes_a_split_and_restores_fades_and_transition() {
        let mut p = proj(&[6000, 5000]);
        let a = p.clips[0].id;
        set_fades(&mut p, a, 500, 700).unwrap();
        set_transition(&mut p, a, Transition::DipToBlack { ms: 400 }).unwrap();
        rename(&mut p, a, "Intro").unwrap();
        let before = p.clone();
        let r1 = split(&mut p, a, 2000).unwrap();
        let r2 = split(&mut p, r1, 4000).unwrap();
        assert_eq!(p.clips.len(), 4);
        // order of ids does not matter; duplicates are fine
        let kept = merge(&mut p, &[r2, a, r1, a]).unwrap();
        assert_eq!(kept, a);
        assert_eq!(p, before, "a full merge is the exact inverse of the splits");
        // merging only the two right-hand pieces leaves the left one alone
        let r1 = split(&mut p, a, 2000).unwrap();
        let r2 = split(&mut p, r1, 4000).unwrap();
        assert_eq!(merge(&mut p, &[r1, r2]).unwrap(), r1);
        assert_eq!(p.clips.len(), 3);
        assert_eq!((p.clips[1].source_start, p.clips[1].source_end, p.clips[1].fade_out), (2000, 6000, 700));
        assert_eq!(p.clips[1].transition_out, Transition::DipToBlack { ms: 400 });
        assert_eq!(p.clips[1].name.as_deref(), Some("Intro"));
        assert_eq!(p.duration_ms(), 10_600, "11 s minus the 400 ms dip overlap");
    }

    #[test]
    fn merge_refuses_what_is_not_a_split() {
        let mut p = proj(&[5000, 4000, 3000]);
        let (a, b, c) = (p.clips[0].id, p.clips[1].id, p.clips[2].id);
        assert!(matches!(merge(&mut p, &[a]), Err(Error::InvalidEdit(m)) if m.contains("two or more")));
        assert!(matches!(merge(&mut p, &[a, a]), Err(Error::InvalidEdit(m)) if m.contains("neighbouring")));
        assert!(matches!(merge(&mut p, &[a, b]), Err(Error::InvalidEdit(m)) if m.contains("same file")));
        assert!(matches!(merge(&mut p, &[a, Uuid::new_v4()]), Err(Error::ClipNotFound(_))));
        // same file, but not consecutive pieces
        let r = split(&mut p, a, 2500).unwrap();
        move_to(&mut p, r, 2).unwrap(); // a, b, r
        assert!(matches!(merge(&mut p, &[a, r]), Err(Error::InvalidEdit(m)) if m.contains("neighbouring")));
        move_to(&mut p, b, 0).unwrap(); // b, a, r
        trim(&mut p, r, 3000, 5000).unwrap();
        assert!(matches!(merge(&mut p, &[a, r]), Err(Error::InvalidEdit(m)) if m.contains("not consecutive")));
        assert!(matches!(merge(&mut p, &[a, c]), Err(Error::InvalidEdit(m)) if m.contains("neighbouring")));
        assert_eq!(p.clips.len(), 4, "a refused merge changes nothing");
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
