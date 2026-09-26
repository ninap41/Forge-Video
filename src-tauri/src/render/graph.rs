//! Translates a `Project` into an ffmpeg `-filter_complex` argv. Pure function; unit-tested by
//! inspecting the generated arguments. The frontend never sees any of this.

use crate::project::{Clip, Ms, Project, Transition};
use crate::render::ffmpeg::ms_to_secs;
use crate::render::planner::ExportSettings;

const AUDIO_FMT: &str = "aformat=sample_fmts=fltp:sample_rates=48000:channel_layouts=stereo";

fn f(ms: Ms) -> String {
    ms_to_secs(ms)
}

fn even(v: f64) -> u32 {
    ((v / 2.0).round() as u32 * 2).max(2)
}

/// crop=w:h:x:y,scale=W:H for this clip given the project crop and output preset.
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
    let scale = p.crop.scale.max(1.0) as f64;
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

fn audio_chain(i: usize, c: &Clip) -> String {
    let d = c.duration_ms();
    if !c.media.has_audio || c.muted {
        return format!("anullsrc=r=48000:cl=stereo:d={}[a{i}]", f(d));
    }
    let mut chain = vec![
        format!("[{i}:a]atrim=start={}:end={}", f(c.source_start), f(c.source_end)),
        "asetpts=PTS-STARTPTS".into(),
        AUDIO_FMT.into(),
        format!("volume={:.3}", c.volume),
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

pub fn build_args(p: &Project, s: &ExportSettings) -> Vec<String> {
    let fps = p.output_fps().as_f64();
    let n = p.clips.len();
    let mut args: Vec<String> = vec!["-y".into()];
    for c in &p.clips {
        args.push("-i".into());
        args.push(c.source.to_string_lossy().into());
    }
    let music_idx = p.music.as_ref().map(|m| {
        args.push("-i".into());
        args.push(m.source.to_string_lossy().into());
        n
    });

    let mut filters: Vec<String> = Vec::new();
    for (i, c) in p.clips.iter().enumerate() {
        if !s.audio_only {
            filters.push(video_chain(p, i, c, fps));
        }
        filters.push(audio_chain(i, c));
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

    // Music bed.
    if let (Some(m), Some(mi)) = (&p.music, music_idx) {
        if !m.muted {
            let d = m.trim_end.saturating_sub(m.trim_start);
            let mut chain = vec![
                format!("[{mi}:a]atrim=start={}:end={}", f(m.trim_start), f(m.trim_end)),
                "asetpts=PTS-STARTPTS".into(),
                AUDIO_FMT.into(),
                format!("volume={:.3}", m.volume),
            ];
            if m.fade_in > 0 {
                chain.push(format!("afade=t=in:st=0:d={}", f(m.fade_in)));
            }
            if m.fade_out > 0 {
                chain.push(format!("afade=t=out:st={}:d={}", f(d.saturating_sub(m.fade_out)), f(m.fade_out)));
            }
            if m.timeline_start > 0 {
                chain.push(format!("adelay=delays={}:all=1", m.timeline_start));
            }
            filters.push(format!("{}[am]", chain.join(",")));
            filters.push(format!("[{cur_a}][am]amix=inputs=2:duration=first:normalize=0[amix]"));
            cur_a = "amix".into();
        }
    }

    filters.push(format!("[{cur_a}]atrim=end={}[aout]", f(cur_len)));
    if !s.audio_only {
        filters.push(format!("[{cur_v}]trim=end={}[vout]", f(cur_len)));
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
            audio_codec: Some("aac".into()), sample_rate: Some(48000), rotation: 0,
        }
    }
    fn settings() -> ExportSettings {
        ExportSettings { destination: PathBuf::from("/tmp/out.mp4"), quality: Quality::Standard, audio_only: false }
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
        p.music = Some(AudioTrack::new(PathBuf::from("/m.m4a"), 8000));
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
            audio_codec: Some("aac".into()), sample_rate: Some(48000), rotation: 0,
        }
    }
    fn settings() -> ExportSettings {
        ExportSettings { destination: PathBuf::from("/tmp/out.mp4"), quality: Quality::Standard, audio_only: false }
    }
    fn filter_of(args: &[String]) -> String {
        let i = args.iter().position(|a| a == "-filter_complex").unwrap();
        args[i + 1].clone()
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
        p.crop = Crop { scale: 0.5, x: 0.5, y: 0.5 };
        assert_eq!(crop_scale_filter(&p, &c), "crop=1920:1080:0:0,scale=1920:1080:flags=bicubic", "scale below 1 is treated as 1");
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
    }

    #[test]
    fn music_bed_with_offset_trim_and_fades() {
        let mut p = two_clips();
        let mut m = AudioTrack::new(PathBuf::from("/m.m4a"), 20_000);
        m.trim_start = 1000;
        m.trim_end = 9000;
        m.timeline_start = 2500;
        m.fade_in = 500;
        m.fade_out = 1000;
        m.volume = 0.3;
        p.music = Some(m);
        let args = build_args(&p, &settings());
        assert_eq!(args.iter().filter(|a| *a == "-i").count(), 3);
        assert_eq!(args[6], "/m.m4a", "music is the last input");
        let fc = filter_of(&args);
        let am = fc.split(';').find(|s| s.ends_with("[am]")).unwrap();
        assert_eq!(
            am,
            "[2:a]atrim=start=1.000:end=9.000,asetpts=PTS-STARTPTS,aformat=sample_fmts=fltp:sample_rates=48000:channel_layouts=stereo,volume=0.300,afade=t=in:st=0:d=0.500,afade=t=out:st=7.000:d=1.000,adelay=delays=2500:all=1[am]"
        );
        assert!(fc.contains("[ax1][am]amix=inputs=2:duration=first:normalize=0[amix]"), "{fc}");
        assert!(fc.contains("[amix]atrim=end=10.000[aout]"), "{fc}");
    }

    #[test]
    fn muted_music_is_still_an_input_but_not_mixed() {
        let mut p = two_clips();
        let mut m = AudioTrack::new(PathBuf::from("/m.m4a"), 20_000);
        m.muted = true;
        p.music = Some(m);
        let args = build_args(&p, &settings());
        assert_eq!(args.iter().filter(|a| *a == "-i").count(), 3);
        let fc = filter_of(&args);
        assert!(!fc.contains("amix"), "{fc}");
        assert!(!fc.contains("[am]"), "{fc}");
        assert!(fc.contains("[ax1]atrim=end=10.000[aout]"), "{fc}");
    }

    #[test]
    fn music_at_timeline_start_zero_has_no_adelay() {
        let mut p = two_clips();
        p.music = Some(AudioTrack::new(PathBuf::from("/m.m4a"), 20_000));
        let fc = filter_of(&build_args(&p, &settings()));
        assert!(!fc.contains("adelay"), "{fc}");
        assert!(fc.contains("volume=0.500"), "default bed level");
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
        p.music = Some(AudioTrack::new(PathBuf::from("/m.m4a"), 20_000));
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
