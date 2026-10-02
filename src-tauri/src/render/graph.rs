//! Translates a `Project` into an ffmpeg `-filter_complex` argv. Pure function; unit-tested by
//! inspecting the generated arguments. The frontend never sees any of this.

use crate::project::{AudioClip, Clip, Ms, OverlayClip, Project, TextClip, Transition};
use std::path::{Path, PathBuf};
use uuid::Uuid;
use crate::render::ffmpeg::ms_to_secs;
use crate::render::planner::ExportSettings;

const AUDIO_FMT: &str = "aformat=sample_fmts=fltp:sample_rates=48000:channel_layouts=stereo";

fn f(ms: Ms) -> String {
    ms_to_secs(ms)
}

fn even(v: f64) -> u32 {
    ((v / 2.0).round() as u32 * 2).max(2)
}

/// The framing stage for this clip given the project crop and output preset: zoomed in,
/// `crop=w:h:x:y,scale=W:H`; zoomed out (scale < 1), the source is shrunk, padded with black on a
/// canvas big enough for any offset, then the frame is cut out so the picture can sit partly outside.
pub fn crop_scale_filter(p: &Project, c: &Clip) -> String {
    let (ow, oh) = p.aspect.dimensions();
    let (sw, sh) = c.media.display_size();
    if sw == 0 || sh == 0 {
        return format!("scale={ow}:{oh}");
    }
    let (sw, sh) = (sw as f64, sh as f64);
    let target = ow as f64 / oh as f64;
    // Largest region of the source with the target aspect, then zoom by `scale`.
    let base_w = sw.min(sh * target);
    let base_h = base_w / target;
    let scale = (p.crop.scale as f64).max(crate::project::CROP_SCALE_MIN as f64);
    if scale < 1.0 {
        // Output pixels per source pixel; the source point (x, y) lands at the frame centre.
        let k = ow as f64 / base_w * scale;
        let (w, h) = (even(sw * k), even(sh * k));
        let ox = ow as f64 / 2.0 - p.crop.x as f64 * sw * k;
        let oy = oh as f64 / 2.0 - p.crop.y as f64 * sh * k;
        return format!(
            "scale={w}:{h}:flags=bicubic,pad={}:{}:{ow}:{oh}:black,crop={ow}:{oh}:{}:{}",
            w + 2 * ow, h + 2 * oh, (ow as f64 - ox).round().clamp(0.0, (w + ow) as f64) as u32, (oh as f64 - oy).round().clamp(0.0, (h + oh) as f64) as u32
        );
    }
    let cw = base_w / scale;
    let ch = base_h / scale;
    let cx = (p.crop.x as f64 * sw).clamp(cw / 2.0, sw - cw / 2.0);
    let cy = (p.crop.y as f64 * sh).clamp(ch / 2.0, sh - ch / 2.0);
    format!(
        "crop={}:{}:{}:{},scale={ow}:{oh}:flags=bicubic",
        even(cw), even(ch), (cx - cw / 2.0).round() as u32, (cy - ch / 2.0).round() as u32
    )
}

fn video_chain(p: &Project, i: usize, c: &Clip, fps: f64) -> String {
    let d = c.duration_ms();
    let mut chain = vec![
        format!("[{i}:v]trim=start={}:end={}", f(c.source_start), f(c.source_end)),
        "setpts=PTS-STARTPTS".into(),
        crop_scale_filter(p, c),
        format!("fps={fps}"),
        "format=yuv420p".into(),
        "setsar=1".into(),
    ];
    if c.fade_in > 0 {
        chain.push(format!("fade=t=in:st=0:d={}", f(c.fade_in)));
    }
    if c.fade_out > 0 {
        chain.push(format!("fade=t=out:st={}:d={}", f(d.saturating_sub(c.fade_out)), f(c.fade_out)));
    }
    format!("{}[v{i}]", chain.join(","))
}

/// Clip volume times the track fader, e.g. `volume=0.500`.
fn gain(clip_volume: f32, track_volume: f32) -> String {
    format!("volume={:.3}", clip_volume * track_volume)
}

fn audio_chain(i: usize, c: &Clip, track_muted: bool, track_volume: f32) -> String {
    let d = c.duration_ms();
    if !c.media.has_audio || c.muted || track_muted {
        return format!("anullsrc=r=48000:cl=stereo:d={}[a{i}]", f(d));
    }
    let mut chain = vec![
        format!("[{i}:a]atrim=start={}:end={}", f(c.source_start), f(c.source_end)),
        "asetpts=PTS-STARTPTS".into(),
        AUDIO_FMT.into(),
        gain(c.volume, track_volume),
    ];
    if c.fade_in > 0 {
        chain.push(format!("afade=t=in:st=0:d={}", f(c.fade_in)));
    }
    if c.fade_out > 0 {
        chain.push(format!("afade=t=out:st={}:d={}", f(d.saturating_sub(c.fade_out)), f(c.fade_out)));
    }
    // Pad so audio is never shorter than video (avoids xfade/acrossfade drift on odd sample counts).
    chain.push(format!("apad=whole_dur={}", f(d)));
    chain.push(format!("atrim=end={}", f(d)));
    format!("{}[a{i}]", chain.join(","))
}

/// ffmpeg inputs in order. V1 clips come first so `[i:v]` == clip index; overlays and audio
/// clips follow, each with whatever pre-input flags they need.
struct Inputs {
    args: Vec<String>,
    next: usize,
}

impl Inputs {
    fn new() -> Self {
        Inputs { args: vec!["-y".into()], next: 0 }
    }
    fn add(&mut self, path: &std::path::Path, pre: &[String]) -> usize {
        self.args.extend_from_slice(pre);
        self.args.push("-i".into());
        self.args.push(path.to_string_lossy().into());
        self.next += 1;
        self.next - 1
    }
}

/// Overlay (V2) video: scaled by placement, alpha fades, then shifted to its timeline position.
fn overlay_chain(p: &Project, input: usize, k: usize, o: &OverlayClip, fps: f64) -> String {
    let (ow, _) = p.aspect.dimensions();
    let d = o.duration_ms();
    let bw = even(ow as f64 * o.placement.scale as f64);
    let mut chain = vec![
        format!("[{input}:v]trim=start={}:end={}", f(o.source_start), f(o.source_end)),
        "setpts=PTS-STARTPTS".into(),
        "format=rgba".into(),
    ];
    // Constant opacity scales the alpha channel while the frame is still RGBA (colorchannelmixer is an RGB filter).
    if o.opacity < 1.0 {
        chain.push(format!("colorchannelmixer=aa={:.3}", o.opacity));
    }
    chain.extend([
        format!("scale={bw}:-2:flags=bicubic"),
        format!("fps={fps}"),
        "format=yuva420p".into(),
        "setsar=1".into(),
    ]);
    if o.fade_in > 0 {
        chain.push(format!("fade=t=in:st=0:d={}:alpha=1", f(o.fade_in)));
    }
    if o.fade_out > 0 {
        chain.push(format!("fade=t=out:st={}:d={}:alpha=1", f(d.saturating_sub(o.fade_out)), f(o.fade_out)));
    }
    chain.push(format!("setpts=PTS+{}/TB", f(o.timeline_start)));
    format!("{}[ov{k}]", chain.join(","))
}

/// What a free-positioned sound needs from its clip: audio clips and overlay clips both qualify.
struct FreeAudio { source_start: Ms, source_end: Ms, timeline_start: Ms, volume: f32, fade_in: Ms, fade_out: Ms }

impl From<&AudioClip> for FreeAudio {
    fn from(c: &AudioClip) -> Self {
        FreeAudio { source_start: c.source_start, source_end: c.source_end, timeline_start: c.timeline_start, volume: c.volume, fade_in: c.fade_in, fade_out: c.fade_out }
    }
}
impl From<&OverlayClip> for FreeAudio {
    /// An overlay's opacity fades double as its audio fades.
    fn from(o: &OverlayClip) -> Self {
        FreeAudio { source_start: o.source_start, source_end: o.source_end, timeline_start: o.timeline_start, volume: o.volume, fade_in: o.fade_in, fade_out: o.fade_out }
    }
}

/// One free-positioned sound: trimmed, shaped, then delayed to its timeline position.
fn free_audio_chain(input: usize, label: &str, c: &FreeAudio, track_volume: f32) -> String {
    let d = c.source_end.saturating_sub(c.source_start);
    let mut chain = vec![
        format!("[{input}:a]atrim=start={}:end={}", f(c.source_start), f(c.source_end)),
        "asetpts=PTS-STARTPTS".into(),
        AUDIO_FMT.into(),
        gain(c.volume, track_volume),
    ];
    if c.fade_in > 0 {
        chain.push(format!("afade=t=in:st=0:d={}", f(c.fade_in)));
    }
    if c.fade_out > 0 {
        chain.push(format!("afade=t=out:st={}:d={}", f(d.saturating_sub(c.fade_out)), f(c.fade_out)));
    }
    if c.timeline_start > 0 {
        chain.push(format!("adelay=delays={}:all=1", c.timeline_start));
    }
    format!("{}[{label}]", chain.join(","))
}

/// Text track (T1): a title already rasterised at output pixel size, alpha fades, shifted to its
/// timeline position. No scaling: the PNG is exactly the box the preview showed.
fn text_chain(input: usize, k: usize, t: &TextClip, fps: f64) -> String {
    let mut chain = vec![
        format!("[{input}:v]format=rgba"),
        format!("fps={fps}"),
        "format=yuva420p".into(),
        "setsar=1".into(),
    ];
    if t.fade_in > 0 {
        chain.push(format!("fade=t=in:st=0:d={}:alpha=1", f(t.fade_in)));
    }
    if t.fade_out > 0 {
        chain.push(format!("fade=t=out:st={}:d={}:alpha=1", f(t.duration.saturating_sub(t.fade_out)), f(t.fade_out)));
    }
    chain.push(format!("setpts=PTS+{}/TB", f(t.timeline_start)));
    format!("{}[tx{k}]", chain.join(","))
}

/// A title's rasterised PNG, produced by the webview for this export.
pub type TextRaster = (Uuid, PathBuf);

/// An overlay is heard when it has a soundtrack and neither it nor its row is muted.
fn overlay_audible(p: &Project, o: &OverlayClip) -> bool {
    let layer_muted = p.overlay_audio.get(o.layer as usize).map(|l| l.muted).unwrap_or(false);
    o.media.has_audio && !o.media.is_still && !o.muted && !layer_muted
}

pub fn build_args(p: &Project, s: &ExportSettings) -> Vec<String> {
    build_args_with_texts(p, s, &[])
}

/// Like `build_args`, with a rasterised PNG per text clip. Titles inside the V1 span that have no
/// raster are skipped (the command layer refuses to start such an export).
pub fn build_args_with_texts(p: &Project, s: &ExportSettings, texts: &[TextRaster]) -> Vec<String> {
    let fps = p.output_fps().as_f64();
    let n = p.clips.len();
    let v1_len = p.duration_ms();
    let mut inputs = Inputs::new();
    for c in &p.clips {
        let pre: Vec<String> = if c.media.is_still {
            ["-loop", "1", "-framerate", &format!("{fps}"), "-t", &f(c.source_end)].map(String::from).to_vec()
        } else { Vec::new() };
        inputs.add(&c.source, &pre);
    }
    // Overlays that never appear inside the V1 span are left out entirely. An audio-only export
    // still needs the ones that are heard.
    let overlays: Vec<(usize, &OverlayClip)> = p.overlays.iter()
        .filter(|o| o.timeline_start < v1_len && (!s.audio_only || overlay_audible(p, o)))
        .map(|o| {
            let pre: Vec<String> = if o.media.is_still {
                ["-loop", "1", "-framerate", &format!("{fps}"), "-t", &f(o.duration_ms())].map(String::from).to_vec()
            } else { Vec::new() };
            (inputs.add(&o.source, &pre), o)
        }).collect();
    // Titles come after every overlay so they composite on top; they are silent, so never in audio-only.
    let raster_of = |t: &TextClip| texts.iter().find(|(id, _)| *id == t.id).map(|(_, path)| path.as_path());
    let titles: Vec<(usize, &TextClip)> = if s.audio_only { Vec::new() } else {
        p.texts.iter().filter(|t| t.timeline_start < v1_len)
            .filter_map(|t| raster_of(t).map(|path: &Path| {
                let pre: Vec<String> = ["-loop", "1", "-framerate", &format!("{fps}"), "-t", &f(t.duration)].map(String::from).to_vec();
                (inputs.add(path, &pre), t)
            })).collect()
    };
    // Every free sound that is heard: overlay soundtracks share their video input; muted audio
    // clips and muted tracks are not even inputs.
    let mut audio: Vec<(usize, FreeAudio, f32)> = overlays.iter()
        .filter(|(_, o)| overlay_audible(p, o))
        .map(|(i, o)| (*i, FreeAudio::from(*o), p.overlay_audio.get(o.layer as usize).map(|l| l.volume).unwrap_or(1.0)))
        .collect();
    for t in p.audio_tracks.iter().filter(|t| !t.muted) {
        for c in t.clips.iter().filter(|c| !c.muted && c.timeline_start < v1_len) {
            audio.push((inputs.add(&c.source, &[]), FreeAudio::from(c), t.volume));
        }
    }
    let mut args = inputs.args;

    let mut filters: Vec<String> = Vec::new();
    for (i, c) in p.clips.iter().enumerate() {
        if !s.audio_only {
            filters.push(video_chain(p, i, c, fps));
        }
        filters.push(audio_chain(i, c, p.video_muted, p.video_volume));
    }

    // Chain clips left to right, honouring transition_out of the left clip.
    let mut cur_v = "v0".to_string();
    let mut cur_a = "a0".to_string();
    let mut cur_len: Ms = p.clips[0].duration_ms();
    for i in 1..n {
        let prev = &p.clips[i - 1];
        let len = p.clips[i].duration_ms();
        let t = prev.transition_out;
        let td = t.duration_ms();
        let (nv, na) = (format!("vx{i}"), format!("ax{i}"));
        match t {
            Transition::None => {
                if !s.audio_only {
                    filters.push(format!("[{cur_v}][v{i}]concat=n=2:v=1:a=0[{nv}]"));
                }
                filters.push(format!("[{cur_a}][a{i}]concat=n=2:v=0:a=1[{na}]"));
                cur_len += len;
            }
            Transition::CrossDissolve { .. } | Transition::DipToBlack { .. } => {
                let kind = if matches!(t, Transition::CrossDissolve { .. }) { "fade" } else { "fadeblack" };
                if !s.audio_only {
                    filters.push(format!(
                        "[{cur_v}][v{i}]xfade=transition={kind}:duration={}:offset={}[{nv}]",
                        f(td), f(cur_len.saturating_sub(td))
                    ));
                }
                filters.push(format!("[{cur_a}][a{i}]acrossfade=d={}:c1=tri:c2=tri[{na}]", f(td)));
                cur_len = cur_len + len - td;
            }
        }
        cur_v = nv;
        cur_a = na;
    }

    // Overlay track, composited on top of the finished V1 chain in timeline time.
    let (ow, oh) = p.aspect.dimensions();
    for (k, (input, o)) in overlays.iter().enumerate().filter(|_| !s.audio_only) {
        filters.push(overlay_chain(p, *input, k, o, fps));
        let (x, y) = (o.placement.x as f64, o.placement.y as f64);
        filters.push(format!(
            "[{cur_v}][ov{k}]overlay=x={}:y={}:eof_action=pass:enable='between(t,{},{})'[vo{k}]",
            format!("{:.0}-w/2", ow as f64 * x), format!("{:.0}-h/2", oh as f64 * y),
            f(o.timeline_start), f(o.end_ms())
        ));
        cur_v = format!("vo{k}");
    }
    for (k, (input, t)) in titles.iter().enumerate() {
        filters.push(text_chain(*input, k, t, fps));
        filters.push(format!(
            "[{cur_v}][tx{k}]overlay=x={}:y={}:eof_action=pass:enable='between(t,{},{})'[vt{k}]",
            format!("{:.0}-w/2", ow as f64 * t.x as f64), format!("{:.0}-h/2", oh as f64 * t.y as f64),
            f(t.timeline_start), f(t.end_ms())
        ));
        cur_v = format!("vt{k}");
    }

    // Audio tracks, mixed under the V1 audio; `duration=first` keeps V1 as the clock.
    if !audio.is_empty() {
        let mut labels = vec![format!("[{cur_a}]")];
        for (k, (input, c, track_volume)) in audio.iter().enumerate() {
            let label = format!("au{k}");
            filters.push(free_audio_chain(*input, &label, c, *track_volume));
            labels.push(format!("[{label}]"));
        }
        filters.push(format!("{}amix=inputs={}:duration=first:normalize=0[amix]", labels.concat(), labels.len()));
        cur_a = "amix".into();
    }

    // A range export cuts the finished graph, so every `enable=between(t,…)` above stays in
    // timeline time; the planner has already validated the span.
    match crate::render::planner::export_span(p, s) {
        Ok((start, end)) if s.range.is_some() => {
            filters.push(format!("[{cur_a}]atrim=start={}:end={},asetpts=PTS-STARTPTS[aout]", f(start), f(end)));
            if !s.audio_only {
                filters.push(format!("[{cur_v}]trim=start={}:end={},setpts=PTS-STARTPTS[vout]", f(start), f(end)));
            }
        }
        _ => {
            filters.push(format!("[{cur_a}]atrim=end={}[aout]", f(cur_len)));
            if !s.audio_only {
                filters.push(format!("[{cur_v}]trim=end={}[vout]", f(cur_len)));
            }
        }
    }

    args.push("-filter_complex".into());
    args.push(filters.join(";"));

    if s.audio_only {
        args.extend(["-map", "[aout]", "-c:a", "aac", "-b:a", "192k", "-movflags", "+faststart"].map(String::from));
    } else {
        let (w, h) = p.aspect.dimensions();
        let kbps = s.quality.bitrate_kbps(w, h);
        args.extend(["-map", "[vout]", "-map", "[aout]"].map(String::from));
        args.extend(["-c:v", "h264_videotoolbox", "-b:v"].map(String::from));
        args.push(format!("{kbps}k"));
        args.extend(["-profile:v", "high", "-pix_fmt", "yuv420p", "-r"].map(String::from));
        args.push(format!("{fps}"));
        args.extend(["-c:a", "aac", "-b:a", "192k", "-movflags", "+faststart"].map(String::from));
    }
    args.push(s.destination.to_string_lossy().into());
    args
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::*;
    use crate::render::planner::Quality;
    use crate::timeline;
    use std::path::PathBuf;

    fn media(w: u32, h: u32) -> MediaInfo {
        MediaInfo {
            duration_ms: 5000, width: w, height: h, fps: Rational { num: 30, den: 1 },
            codec: "h264".into(), container: "mov,mp4".into(), has_audio: true,
            audio_codec: Some("aac".into()), sample_rate: Some(48000), rotation: 0, is_still: false,
        }
    }
    fn settings() -> ExportSettings {
        ExportSettings { destination: PathBuf::from("/tmp/out.mp4"), quality: Quality::Standard, audio_only: false, range: None }
    }
    fn add_music(p: &mut Project, dur: Ms, at: Ms) -> uuid::Uuid {
        let t = timeline::audio_track_add(p, "Music");
        let mut m = media(0, 0);
        m.duration_ms = dur;
        timeline::audio_clip_add(p, t, AudioClip::new(PathBuf::from("/m.m4a"), m), at).unwrap()
    }
    fn filter_of(args: &[String]) -> String {
        let i = args.iter().position(|a| a == "-filter_complex").unwrap();
        args[i + 1].clone()
    }

    #[test]
    fn crop_centre_fit_for_vertical_from_landscape() {
        let mut p = Project::new("t");
        p.aspect = AspectPreset::Shorts9x16;
        let c = Clip::new(PathBuf::from("/a.mp4"), media(1920, 1080));
        // 9:16 region inside 1920x1080 → 607.5x1080 → even 608x1080, x = (1920-607.5)/2 ≈ 656
        assert_eq!(crop_scale_filter(&p, &c), "crop=608:1080:656:0,scale=1080:1920:flags=bicubic");
        p.crop = Crop { scale: 2.0, x: 0.0, y: 0.5 };
        // zoomed: 303.75x540, centre clamped to left edge → x=0, y=270
        assert_eq!(crop_scale_filter(&p, &c), "crop=304:540:0:270,scale=1080:1920:flags=bicubic");
    }

    #[test]
    fn rotated_source_uses_display_size() {
        let mut p = Project::new("t");
        p.aspect = AspectPreset::Shorts9x16;
        let mut m = media(1920, 1080);
        m.rotation = 90;
        let c = Clip::new(PathBuf::from("/a.mp4"), m);
        // displayed 1080x1920 already 9:16 → full frame crop
        assert_eq!(crop_scale_filter(&p, &c), "crop=1080:1920:0:0,scale=1080:1920:flags=bicubic");
    }

    #[test]
    fn dissolve_and_cut_generate_expected_graph() {
        let mut p = Project::new("t");
        for i in 0..3 {
            timeline::append(&mut p, Clip::new(PathBuf::from(format!("/{i}.mp4")), media(1920, 1080)));
        }
        let a = p.clips[0].id;
        timeline::set_transition(&mut p, a, Transition::CrossDissolve { ms: 1000 }).unwrap();
        timeline::set_fades(&mut p, a, 500, 0).unwrap();
        add_music(&mut p, 8000, 0);
        let args = build_args(&p, &settings());
        let fc = filter_of(&args);
        assert!(fc.contains("[v0]"), "{fc}");
        assert!(fc.contains("fade=t=in:st=0:d=0.500"), "{fc}");
        assert!(fc.contains("[v0][v1]xfade=transition=fade:duration=1.000:offset=4.000[vx1]"), "{fc}");
        assert!(fc.contains("[a0][a1]acrossfade=d=1.000"), "{fc}");
        assert!(fc.contains("[vx1][v2]concat=n=2:v=1:a=0[vx2]"), "{fc}");
        assert!(fc.contains("[3:a]atrim"), "{fc}");
        assert!(fc.contains("amix=inputs=2"), "{fc}");
        assert!(fc.ends_with("[vx2]trim=end=14.000[vout]"), "{fc}");
        assert_eq!(args.iter().filter(|a| *a == "-i").count(), 4);
        assert!(args.contains(&"h264_videotoolbox".into()));
        assert!(args.contains(&"10000k".into()));
    }

    #[test]
    fn muting_the_video_track_silences_every_clip() {
        let mut p = Project::new("t");
        for i in 0..2 { timeline::append(&mut p, Clip::new(PathBuf::from(format!("/{i}.mp4")), media(1920, 1080))); }
        p.video_muted = true;
        let fc = filter_of(&build_args(&p, &settings()));
        assert!(fc.contains("anullsrc=r=48000:cl=stereo:d=5.000[a0]"), "{fc}");
        assert!(fc.contains("anullsrc=r=48000:cl=stereo:d=5.000[a1]"), "{fc}");
        assert!(!fc.contains("[0:a]") && !fc.contains("[1:a]"), "{fc}");
    }

    #[test]
    fn muted_clip_gets_silence() {
        let mut p = Project::new("t");
        timeline::append(&mut p, Clip::new(PathBuf::from("/a.mp4"), media(1280, 720)));
        let id = p.clips[0].id;
        timeline::set_volume(&mut p, id, 1.0, true).unwrap();
        let fc = filter_of(&build_args(&p, &settings()));
        assert!(fc.contains("anullsrc=r=48000:cl=stereo:d=5.000[a0]"), "{fc}");
    }

    #[test]
    fn audio_only_has_no_video_maps() {
        let mut p = Project::new("t");
        timeline::append(&mut p, Clip::new(PathBuf::from("/a.mp4"), media(1280, 720)));
        let mut s = settings();
        s.audio_only = true;
        let args = build_args(&p, &s);
        assert!(!args.contains(&"[vout]".into()));
        assert!(!filter_of(&args).contains("[v0]"));
    }
}

#[cfg(test)]
mod more_tests {
    use super::*;
    use crate::project::*;
    use crate::render::planner::Quality;
    use crate::timeline;
    use std::path::PathBuf;

    fn media(w: u32, h: u32) -> MediaInfo {
        MediaInfo {
            duration_ms: 5000, width: w, height: h, fps: Rational { num: 30, den: 1 },
            codec: "h264".into(), container: "mov,mp4".into(), has_audio: true,
            audio_codec: Some("aac".into()), sample_rate: Some(48000), rotation: 0, is_still: false,
        }
    }
    fn settings() -> ExportSettings {
        ExportSettings { destination: PathBuf::from("/tmp/out.mp4"), quality: Quality::Standard, audio_only: false, range: None }
    }
    fn filter_of(args: &[String]) -> String {
        let i = args.iter().position(|a| a == "-filter_complex").unwrap();
        args[i + 1].clone()
    }
    fn audio_media(dur: Ms) -> MediaInfo { let mut m = media(0, 0); m.duration_ms = dur; m }
    fn still_media() -> MediaInfo {
        let mut m = media(400, 300); m.is_still = true; m.has_audio = false; m.duration_ms = STILL_DEFAULT_MS; m
    }
    fn two_clips() -> Project {
        let mut p = Project::new("t");
        timeline::append(&mut p, Clip::new(PathBuf::from("/a.mp4"), media(1920, 1080)));
        timeline::append(&mut p, Clip::new(PathBuf::from("/b.mp4"), media(1920, 1080)));
        p
    }

    #[test]
    fn even_rounds_to_nearest_even_with_a_floor() {
        assert_eq!(even(607.5), 608);
        assert_eq!(even(303.75), 304);
        assert_eq!(even(1080.0), 1080);
        assert_eq!(even(0.0), 2);
        assert_eq!(even(1.0), 2);
    }

    #[test]
    fn crop_filter_for_matching_aspect_is_full_frame() {
        let p = Project::new("t");
        let c = Clip::new(PathBuf::from("/a.mp4"), media(1920, 1080));
        assert_eq!(crop_scale_filter(&p, &c), "crop=1920:1080:0:0,scale=1920:1080:flags=bicubic");
        // smaller source is upscaled, not cropped
        let c = Clip::new(PathBuf::from("/a.mp4"), media(1280, 720));
        assert_eq!(crop_scale_filter(&p, &c), "crop=1280:720:0:0,scale=1920:1080:flags=bicubic");
    }

    #[test]
    fn crop_filter_for_square_and_linkedin_from_landscape() {
        let mut p = Project::new("t");
        let c = Clip::new(PathBuf::from("/a.mp4"), media(1920, 1080));
        p.aspect = AspectPreset::Square1x1;
        assert_eq!(crop_scale_filter(&p, &c), "crop=1080:1080:420:0,scale=1080:1080:flags=bicubic");
        p.aspect = AspectPreset::LinkedIn4x5;
        // 4:5 inside 1920x1080 → 864x1080, x = (1920-864)/2 = 528
        assert_eq!(crop_scale_filter(&p, &c), "crop=864:1080:528:0,scale=1080:1350:flags=bicubic");
    }

    #[test]
    fn crop_filter_for_landscape_output_from_vertical_source_letterboxes_by_cropping_height() {
        let p = Project::new("t");
        let c = Clip::new(PathBuf::from("/a.mp4"), media(1080, 1920));
        // 16:9 region inside 1080x1920 → 1080x607.5 → 1080x608, y = (1920-607.5)/2 ≈ 656
        assert_eq!(crop_scale_filter(&p, &c), "crop=1080:608:0:656,scale=1920:1080:flags=bicubic");
    }

    #[test]
    fn crop_zoom_and_reposition_are_clamped_inside_the_source() {
        let mut p = Project::new("t");
        let c = Clip::new(PathBuf::from("/a.mp4"), media(1920, 1080));
        p.crop = Crop { scale: 2.0, x: 1.0, y: 1.0 };
        // 960x540 window pinned to the bottom-right corner
        assert_eq!(crop_scale_filter(&p, &c), "crop=960:540:960:540,scale=1920:1080:flags=bicubic");
    }

    #[test]
    fn zooming_out_shrinks_the_source_onto_black_and_lets_it_leave_the_frame() {
        let mut p = Project::new("t");
        let c = Clip::new(PathBuf::from("/a.mp4"), media(1920, 1080));
        // half size, centred: 960x540 on a 1920x1080 frame, padded canvas 4800x2700, frame cut at (1440, 810)
        p.crop = Crop { scale: 0.5, x: 0.5, y: 0.5 };
        assert_eq!(crop_scale_filter(&p, &c), "scale=960:540:flags=bicubic,pad=4800:2700:1920:1080:black,crop=1920:1080:1440:810");
        // x = 0: the source's left edge sits at the frame centre, so half of it hangs off the right
        p.crop = Crop { scale: 0.5, x: 0.0, y: 0.5 };
        assert_eq!(crop_scale_filter(&p, &c), "scale=960:540:flags=bicubic,pad=4800:2700:1920:1080:black,crop=1920:1080:960:810");
        // x = 1: the right edge at the centre, half off the left
        p.crop = Crop { scale: 0.5, x: 1.0, y: 1.0 };
        assert_eq!(crop_scale_filter(&p, &c), "scale=960:540:flags=bicubic,pad=4800:2700:1920:1080:black,crop=1920:1080:1920:1080");
        // 9:16 preset: the base window is 607.5 wide, so k = 1080 / 607.5 × 0.25
        p.aspect = AspectPreset::Shorts9x16;
        p.crop = Crop { scale: 0.25, x: 0.5, y: 0.5 };
        assert_eq!(crop_scale_filter(&p, &c), "scale=854:480:flags=bicubic,pad=3014:4320:1080:1920:black,crop=1080:1920:967:1200");
        // anything below the floor behaves like the floor
        p.crop = Crop { scale: 0.01, x: 0.5, y: 0.5 };
        assert_eq!(crop_scale_filter(&p, &c), "scale=854:480:flags=bicubic,pad=3014:4320:1080:1920:black,crop=1080:1920:967:1200");
    }

    #[test]
    fn unknown_source_size_falls_back_to_plain_scale() {
        let p = Project::new("t");
        let c = Clip::new(PathBuf::from("/a.mp4"), media(0, 0));
        assert_eq!(crop_scale_filter(&p, &c), "scale=1920:1080");
    }

    #[test]
    fn hard_cut_uses_concat_for_video_and_audio() {
        let p = two_clips();
        let fc = filter_of(&build_args(&p, &settings()));
        assert!(fc.contains("[v0][v1]concat=n=2:v=1:a=0[vx1]"), "{fc}");
        assert!(fc.contains("[a0][a1]concat=n=2:v=0:a=1[ax1]"), "{fc}");
        assert!(fc.ends_with("[vx1]trim=end=10.000[vout]"), "{fc}");
        assert!(fc.contains("[ax1]atrim=end=10.000[aout]"), "{fc}");
        assert!(!fc.contains("xfade"));
    }

    #[test]
    fn dip_to_black_uses_fadeblack_transition() {
        let mut p = two_clips();
        let a = p.clips[0].id;
        timeline::set_transition(&mut p, a, Transition::DipToBlack { ms: 600 }).unwrap();
        let fc = filter_of(&build_args(&p, &settings()));
        assert!(fc.contains("xfade=transition=fadeblack:duration=0.600:offset=4.400[vx1]"), "{fc}");
        assert!(fc.contains("acrossfade=d=0.600:c1=tri:c2=tri[ax1]"), "{fc}");
        assert!(fc.ends_with("trim=end=9.400[vout]"), "{fc}");
    }

    #[test]
    fn video_chain_has_trim_crop_fps_and_format_in_order() {
        let mut p = Project::new("t");
        timeline::append(&mut p, Clip::new(PathBuf::from("/a.mp4"), media(1920, 1080)));
        let id = p.clips[0].id;
        timeline::trim(&mut p, id, 500, 4500).unwrap();
        timeline::set_fades(&mut p, id, 250, 1000).unwrap();
        let fc = filter_of(&build_args(&p, &settings()));
        let v0 = fc.split(';').find(|s| s.ends_with("[v0]")).unwrap();
        assert_eq!(
            v0,
            "[0:v]trim=start=0.500:end=4.500,setpts=PTS-STARTPTS,crop=1920:1080:0:0,scale=1920:1080:flags=bicubic,fps=30,format=yuv420p,setsar=1,fade=t=in:st=0:d=0.250,fade=t=out:st=3.000:d=1.000[v0]"
        );
        let a0 = fc.split(';').find(|s| s.ends_with("[a0]")).unwrap();
        assert!(a0.starts_with("[0:a]atrim=start=0.500:end=4.500,asetpts=PTS-STARTPTS,"), "{a0}");
        assert!(a0.contains("volume=1.000,afade=t=in:st=0:d=0.250,afade=t=out:st=3.000:d=1.000,apad=whole_dur=4.000,atrim=end=4.000[a0]"), "{a0}");
    }

    #[test]
    fn source_without_audio_gets_silence_of_the_right_length() {
        let mut p = Project::new("t");
        let mut m = media(1920, 1080);
        m.has_audio = false;
        timeline::append(&mut p, Clip::new(PathBuf::from("/a.mp4"), m));
        let id = p.clips[0].id;
        timeline::trim(&mut p, id, 0, 1500).unwrap();
        let fc = filter_of(&build_args(&p, &settings()));
        assert!(fc.contains("anullsrc=r=48000:cl=stereo:d=1.500[a0]"), "{fc}");
        assert!(!fc.contains("[0:a]"));
    }

    #[test]
    fn clip_volume_is_rendered_with_three_decimals() {
        let mut p = Project::new("t");
        timeline::append(&mut p, Clip::new(PathBuf::from("/a.mp4"), media(1920, 1080)));
        let id = p.clips[0].id;
        timeline::set_volume(&mut p, id, 0.25, false).unwrap();
        let fc = filter_of(&build_args(&p, &settings()));
        assert!(fc.contains("volume=0.250"), "{fc}");
        // the V1 fader multiplies the clip volume
        p.video_volume = 0.5;
        let fc = filter_of(&build_args(&p, &settings()));
        assert!(fc.contains("volume=0.125"), "{fc}");
    }

    #[test]
    fn audio_track_fader_scales_its_clips() {
        let mut p = two_clips();
        let t = timeline::audio_track_add(&mut p, "Music");
        let id = timeline::audio_clip_add(&mut p, t, AudioClip::new(PathBuf::from("/m.m4a"), audio_media(20_000)), 0).unwrap();
        timeline::audio_clip_set(&mut p, id, 0.8, 0, 0, false).unwrap();
        timeline::audio_track_update(&mut p, t, "Music", false, 0.25).unwrap();
        let fc = filter_of(&build_args(&p, &settings()));
        assert!(fc.contains("volume=0.200"), "{fc}");
        assert!(fc.contains("amix"), "a turned-down track is still mixed: {fc}");
    }

    #[test]
    fn audio_clip_with_offset_trim_and_fades() {
        let mut p = two_clips();
        let t = timeline::audio_track_add(&mut p, "Music");
        let id = timeline::audio_clip_add(&mut p, t, AudioClip::new(PathBuf::from("/m.m4a"), audio_media(20_000)), 2500).unwrap();
        timeline::audio_clip_trim(&mut p, id, 1000, 9000).unwrap();
        timeline::audio_clip_set(&mut p, id, 0.3, 500, 1000, false).unwrap();
        let args = build_args(&p, &settings());
        assert_eq!(args.iter().filter(|a| *a == "-i").count(), 3);
        assert_eq!(args[6], "/m.m4a", "audio clips come after the V1 clips");
        let fc = filter_of(&args);
        let am = fc.split(';').find(|s| s.ends_with("[au0]")).unwrap();
        assert_eq!(
            am,
            "[2:a]atrim=start=1.000:end=9.000,asetpts=PTS-STARTPTS,aformat=sample_fmts=fltp:sample_rates=48000:channel_layouts=stereo,volume=0.300,afade=t=in:st=0:d=0.500,afade=t=out:st=7.000:d=1.000,adelay=delays=2500:all=1[au0]"
        );
        assert!(fc.contains("[ax1][au0]amix=inputs=2:duration=first:normalize=0[amix]"), "{fc}");
        assert!(fc.contains("[amix]atrim=end=10.000[aout]"), "{fc}");
    }

    #[test]
    fn muted_clips_and_muted_tracks_are_not_inputs() {
        let mut p = two_clips();
        let t = timeline::audio_track_add(&mut p, "Music");
        let id = timeline::audio_clip_add(&mut p, t, AudioClip::new(PathBuf::from("/m.m4a"), audio_media(20_000)), 0).unwrap();
        timeline::audio_clip_set(&mut p, id, 1.0, 0, 0, true).unwrap();
        let args = build_args(&p, &settings());
        assert_eq!(args.iter().filter(|a| *a == "-i").count(), 2);
        let fc = filter_of(&args);
        assert!(!fc.contains("amix"), "{fc}");
        assert!(fc.contains("[ax1]atrim=end=10.000[aout]"), "{fc}");
        // unmute the clip, mute the track
        timeline::audio_clip_set(&mut p, id, 1.0, 0, 0, false).unwrap();
        timeline::audio_track_update(&mut p, t, "Music", true, 1.0).unwrap();
        let args = build_args(&p, &settings());
        assert_eq!(args.iter().filter(|a| *a == "-i").count(), 2);
        assert!(!filter_of(&args).contains("amix"));
        // a clip that starts after V1 ends is also dropped
        timeline::audio_track_update(&mut p, t, "Music", false, 1.0).unwrap();
        timeline::audio_clip_move(&mut p, id, t, 10_000).unwrap();
        assert_eq!(build_args(&p, &settings()).iter().filter(|a| *a == "-i").count(), 2);
    }

    #[test]
    fn audio_clip_at_zero_has_no_adelay_and_three_clips_make_a_four_way_mix() {
        let mut p = two_clips();
        let t = timeline::audio_track_add(&mut p, "Music");
        timeline::audio_clip_add(&mut p, t, AudioClip::new(PathBuf::from("/m.m4a"), audio_media(20_000)), 0).unwrap();
        let fc = filter_of(&build_args(&p, &settings()));
        assert!(!fc.contains("adelay"), "{fc}");
        assert!(fc.contains("volume=1.000"), "{fc}");
        let sfx = timeline::audio_track_add(&mut p, "SFX");
        timeline::audio_clip_add(&mut p, sfx, AudioClip::new(PathBuf::from("/x.wav"), audio_media(500)), 1000).unwrap();
        timeline::audio_clip_add(&mut p, sfx, AudioClip::new(PathBuf::from("/y.wav"), audio_media(500)), 3000).unwrap();
        let args = build_args(&p, &settings());
        assert_eq!(args.iter().filter(|a| *a == "-i").count(), 5);
        let fc = filter_of(&args);
        assert!(fc.contains("[ax1][au0][au1][au2]amix=inputs=4:duration=first:normalize=0[amix]"), "{fc}");
        assert!(fc.contains("adelay=delays=3000:all=1[au2]"), "{fc}");
    }

    #[test]
    fn video_overlay_is_composited_after_the_v1_chain_in_timeline_time() {
        let mut p = two_clips();
        let a = p.clips[0].id;
        timeline::set_transition(&mut p, a, Transition::CrossDissolve { ms: 1000 }).unwrap();
        let id = timeline::overlay_add(&mut p, OverlayClip::new(PathBuf::from("/o.mp4"), media(1280, 720)), 2000, 0);
        timeline::overlay_trim(&mut p, id, 500, 2500).unwrap();
        timeline::overlay_set_fades(&mut p, id, 250, 250).unwrap();
        let args = build_args(&p, &settings());
        assert_eq!(args.iter().filter(|a| *a == "-i").count(), 3);
        assert!(!args.contains(&"-loop".into()));
        let fc = filter_of(&args);
        let ov = fc.split(';').find(|s| s.ends_with("[ov0]")).unwrap();
        assert_eq!(
            ov,
            "[2:v]trim=start=0.500:end=2.500,setpts=PTS-STARTPTS,format=rgba,scale=1920:-2:flags=bicubic,fps=30,format=yuva420p,setsar=1,fade=t=in:st=0:d=0.250:alpha=1,fade=t=out:st=1.750:d=0.250:alpha=1,setpts=PTS+2.000/TB[ov0]"
        );
        assert!(fc.contains("[vx1][ov0]overlay=x=960-w/2:y=540-h/2:eof_action=pass:enable='between(t,2.000,4.000)'[vo0]"), "{fc}");
        assert!(fc.ends_with("[vo0]trim=end=9.000[vout]"), "{fc}");
        // audio-only exports skip the overlay picture; its soundtrack (heard by default) still comes in
        let mut s = settings(); s.audio_only = true;
        let args = build_args(&p, &s);
        assert_eq!(args.iter().filter(|a| *a == "-i").count(), 3);
        assert!(!filter_of(&args).contains("overlay=") && filter_of(&args).contains("amix"));
        timeline::overlay_set_audio(&mut p, id, 1.0, true).unwrap();
        let args = build_args(&p, &s);
        assert_eq!(args.iter().filter(|a| *a == "-i").count(), 2, "a muted overlay is ignored completely");
    }

    #[test]
    fn still_on_v1_is_a_looped_input_with_silence_and_layers_composite_in_order() {
        let mut p = Project::new("s");
        timeline::append(&mut p, Clip::new(PathBuf::from("/title.png"), still_media()));
        timeline::append(&mut p, Clip::new(PathBuf::from("/a.mp4"), media(1920, 1080)));
        let t = p.clips[0].id;
        timeline::trim(&mut p, t, 0, 3000).unwrap();
        let hi = timeline::overlay_add(&mut p, OverlayClip::new(PathBuf::from("/hi.png"), still_media()), 0, 1);
        let lo = timeline::overlay_add(&mut p, OverlayClip::new(PathBuf::from("/lo.png"), still_media()), 0, 0);
        let args = build_args(&p, &settings());
        assert_eq!(&args[1..9], &["-loop", "1", "-framerate", "30", "-t", "3.000", "-i", "/title.png"]);
        let fc = filter_of(&args);
        assert!(fc.contains("anullsrc=r=48000:cl=stereo:d=3.000[a0]"), "{fc}");
        assert!(fc.contains("[0:v]trim=start=0.000:end=3.000"), "{fc}");
        // lower layer first, then the higher one on top of it
        assert!(fc.contains("[vx1][ov0]overlay"), "{fc}");
        assert!(fc.contains("[vo0][ov1]overlay"), "{fc}");
        let i_lo = args.iter().position(|a| a == "/lo.png").unwrap();
        let i_hi = args.iter().position(|a| a == "/hi.png").unwrap();
        assert!(i_lo < i_hi, "inputs follow layer order");
        let _ = (hi, lo);
    }

    #[test]
    fn overlay_opacity_scales_alpha_before_the_fades() {
        let mut p = two_clips();
        let id = timeline::overlay_add(&mut p, OverlayClip::new(PathBuf::from("/o.mp4"), media(1280, 720)), 0, 0);
        timeline::overlay_set_fades(&mut p, id, 250, 0).unwrap();
        timeline::overlay_set_opacity(&mut p, id, 0.5).unwrap();
        let fc = filter_of(&build_args(&p, &settings()));
        assert!(fc.contains("format=rgba,colorchannelmixer=aa=0.500,scale=1920:-2:flags=bicubic"), "{fc}");
        assert!(fc.contains("setsar=1,fade=t=in:st=0:d=0.250:alpha=1"), "{fc}");
        timeline::overlay_set_opacity(&mut p, id, 1.0).unwrap();
        assert!(!filter_of(&build_args(&p, &settings())).contains("colorchannelmixer"));
    }

    #[test]
    fn overlay_soundtrack_is_mixed_in_unless_clip_row_or_media_is_silent() {
        let mut p = two_clips();
        let id = timeline::overlay_add(&mut p, OverlayClip::new(PathBuf::from("/o.mp4"), media(1280, 720)), 2000, 0);
        timeline::overlay_trim(&mut p, id, 500, 2500).unwrap();
        timeline::overlay_set_fades(&mut p, id, 250, 0).unwrap();
        timeline::overlay_set_audio(&mut p, id, 0.5, false).unwrap();
        timeline::overlay_layer_set_audio(&mut p, 0, false, 0.5).unwrap();
        let args = build_args(&p, &settings());
        assert_eq!(args.iter().filter(|a| *a == "-i").count(), 3, "the overlay is one input for both picture and sound");
        let fc = filter_of(&args);
        assert!(fc.contains("[2:a]atrim=start=0.500:end=2.500,asetpts=PTS-STARTPTS,aformat=sample_fmts=fltp:sample_rates=48000:channel_layouts=stereo,volume=0.250,afade=t=in:st=0:d=0.250,adelay=delays=2000:all=1[au0]"), "{fc}");
        assert!(fc.contains("[au0]amix=inputs=2"), "{fc}");
        // muted clip, muted row, or a silent file: picture only
        timeline::overlay_set_audio(&mut p, id, 0.5, true).unwrap();
        assert!(!filter_of(&build_args(&p, &settings())).contains("amix"));
        timeline::overlay_set_audio(&mut p, id, 0.5, false).unwrap();
        timeline::overlay_layer_set_audio(&mut p, 0, true, 1.0).unwrap();
        assert!(!filter_of(&build_args(&p, &settings())).contains("amix"));
        timeline::overlay_layer_set_audio(&mut p, 0, false, 1.0).unwrap();
        p.overlays[0].media.has_audio = false;
        let fc = filter_of(&build_args(&p, &settings()));
        assert!(!fc.contains("amix") && fc.contains("overlay="), "{fc}");
        // an audio-only export keeps the heard overlay and skips its picture
        p.overlays[0].media.has_audio = true;
        let mut s = settings();
        s.audio_only = true;
        let fc = filter_of(&build_args(&p, &s));
        assert!(fc.contains("amix") && !fc.contains("overlay="), "{fc}");
        p.overlays[0].muted = true;
        assert_eq!(build_args(&p, &s).iter().filter(|a| *a == "-i").count(), 2, "a silent overlay is not an input for audio-only");
    }

    #[test]
    fn still_overlay_loops_the_image_and_is_placed_by_its_badge() {
        let mut p = two_clips();
        let id = timeline::overlay_add(&mut p, OverlayClip::new(PathBuf::from("/l.png"), still_media()), 1000, 0);
        timeline::overlay_trim(&mut p, id, 0, 3000).unwrap();
        let args = build_args(&p, &settings());
        let i = args.iter().position(|a| a == "-loop").unwrap();
        assert_eq!(&args[i..i + 8], &["-loop", "1", "-framerate", "30", "-t", "3.000", "-i", "/l.png"]);
        let fc = filter_of(&args);
        assert!(fc.contains("[2:v]trim=start=0.000:end=3.000,setpts=PTS-STARTPTS,format=rgba,scale=672:-2"), "{fc}");
        assert!(fc.contains("overlay=x=1632-w/2:y=918-h/2:eof_action=pass:enable='between(t,1.000,4.000)'"), "{fc}");
        // an overlay starting after V1 ends is skipped
        timeline::overlay_move(&mut p, id, 10_000, 0).unwrap();
        assert_eq!(build_args(&p, &settings()).iter().filter(|a| *a == "-i").count(), 2);
    }

    #[test]
    fn text_raster_is_looped_above_every_overlay_and_never_in_audio_only() {
        let mut p = two_clips();
        timeline::overlay_add(&mut p, OverlayClip::new(PathBuf::from("/l.png"), still_media()), 0, 0);
        let mut t = TextClip::new("Hi");
        t.fade_in = 250; t.fade_out = 500; t.x = 0.5; t.y = 0.9;
        let id = timeline::text_add(&mut p, t, 1000, 0);
        timeline::text_trim(&mut p, id, 3000).unwrap();
        // no raster → the title is skipped, not an error
        assert_eq!(build_args(&p, &settings()).iter().filter(|a| *a == "-i").count(), 3);
        let rasters = vec![(id, PathBuf::from("/tmp/t.png"))];
        let args = build_args_with_texts(&p, &settings(), &rasters);
        let inputs: Vec<&String> = args.iter().enumerate().filter(|(_, a)| *a == "-i").map(|(i, _)| &args[i + 1]).collect();
        assert_eq!(inputs, ["/a.mp4", "/b.mp4", "/l.png", "/tmp/t.png"], "title after the overlay");
        let i = args.iter().position(|a| a == "/tmp/t.png").unwrap();
        assert_eq!(&args[i - 7..i], &["-loop", "1", "-framerate", "30", "-t", "3.000", "-i"]);
        let fc = filter_of(&args);
        assert!(fc.contains("[3:v]format=rgba,fps=30,format=yuva420p,setsar=1,fade=t=in:st=0:d=0.250:alpha=1,fade=t=out:st=2.500:d=0.500:alpha=1,setpts=PTS+1.000/TB[tx0]"), "{fc}");
        assert!(fc.contains("[vo0][tx0]overlay=x=960-w/2:y=972-h/2:eof_action=pass:enable='between(t,1.000,4.000)'[vt0]"), "{fc}");
        assert!(fc.contains("[vt0]trim=end="), "the title is the last video stage");
        let mut a = settings(); a.audio_only = true;
        assert_eq!(build_args_with_texts(&p, &a, &rasters).iter().filter(|x| *x == "-i").count(), 2);
        // a title after V1 ends is left out
        timeline::text_move(&mut p, id, 20_000, 0).unwrap();
        assert_eq!(build_args_with_texts(&p, &settings(), &rasters).iter().filter(|a| *a == "-i").count(), 3);
    }

    #[test]
    fn range_trims_the_finished_graph_and_resets_timestamps() {
        let p = two_clips();
        let mut s = settings();
        s.range = Some(crate::project::Range { start: 1000, end: 3000 });
        let fc = filter_of(&build_args(&p, &s));
        assert!(fc.ends_with("atrim=start=1.000:end=3.000,asetpts=PTS-STARTPTS[aout];[vx1]trim=start=1.000:end=3.000,setpts=PTS-STARTPTS[vout]"), "{fc}");
        s.audio_only = true;
        let fc = filter_of(&build_args(&p, &s));
        assert!(fc.ends_with("atrim=start=1.000:end=3.000,asetpts=PTS-STARTPTS[aout]"), "{fc}");
        assert!(!fc.contains("[vout]"));
        let fc = filter_of(&build_args(&p, &settings()));
        assert!(fc.ends_with("[ax1]atrim=end=10.000[aout];[vx1]trim=end=10.000[vout]"), "no range: the old tail: {fc}");
    }

    #[test]
    fn encoder_args_follow_quality_preset_and_fps() {
        let mut p = two_clips();
        p.fps = Some(Rational { num: 60000, den: 1001 });
        p.aspect = AspectPreset::Square1x1;
        let mut s = settings();
        s.quality = Quality::Draft;
        let args = build_args(&p, &s);
        let i = args.iter().position(|a| a == "-b:v").unwrap();
        assert_eq!(args[i + 1], "2250k", "draft at 1080x1080");
        let r = args.iter().position(|a| a == "-r").unwrap();
        assert!(args[r + 1].starts_with("59.94"), "{}", args[r + 1]);
        assert!(filter_of(&args).contains("fps=59.94"));
        for pair in [("-c:v", "h264_videotoolbox"), ("-profile:v", "high"), ("-pix_fmt", "yuv420p"), ("-c:a", "aac"), ("-b:a", "192k"), ("-movflags", "+faststart")] {
            assert!(args.windows(2).any(|w| w[0] == pair.0 && w[1] == pair.1), "{pair:?}");
        }
        assert_eq!(args.last().unwrap(), "/tmp/out.mp4");
    }

    #[test]
    fn audio_only_graph_still_honours_transitions_and_music() {
        let mut p = two_clips();
        let a = p.clips[0].id;
        timeline::set_transition(&mut p, a, Transition::CrossDissolve { ms: 1000 }).unwrap();
        let t = timeline::audio_track_add(&mut p, "Music");
        timeline::audio_clip_add(&mut p, t, AudioClip::new(PathBuf::from("/m.m4a"), audio_media(20_000)), 0).unwrap();
        let mut s = settings();
        s.audio_only = true;
        let args = build_args(&p, &s);
        let fc = filter_of(&args);
        assert!(fc.contains("acrossfade"), "{fc}");
        assert!(fc.contains("amix"), "{fc}");
        assert!(!fc.contains("concat=n=2:v=1"), "{fc}");
        assert!(!fc.contains("xfade"), "{fc}");
        assert!(!fc.contains("[0:v]"), "{fc}");
        assert!(fc.ends_with("[aout]"), "{fc}");
        assert!(!args.iter().any(|a| a == "-c:v" || a == "-b:v" || a == "-r"));
        assert!(args.windows(2).any(|w| w[0] == "-map" && w[1] == "[aout]"));
    }
}
