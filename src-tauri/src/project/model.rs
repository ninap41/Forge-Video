//! Domain model. Rust is the source of truth; the frontend mirrors these types in `src/types/project.ts`.
//! All times are integer milliseconds (`Ms`). Editing only mutates this data — it never touches media.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

pub type Ms = u64;

pub const PROJECT_FILE_VERSION: u32 = 2;
/// Length a still image gets when it is first placed on the overlay track.
pub const STILL_DEFAULT_MS: Ms = 5000;

fn one() -> u32 { 1 }
fn full() -> f32 { 1.0 }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rational {
    pub num: u32,
    pub den: u32,
}

impl Rational {
    pub fn as_f64(&self) -> f64 {
        if self.den == 0 { 0.0 } else { self.num as f64 / self.den as f64 }
    }
    /// Duration of one frame in ms (fractional).
    pub fn frame_ms(&self) -> f64 {
        if self.num == 0 { 0.0 } else { 1000.0 * self.den as f64 / self.num as f64 }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaInfo {
    pub duration_ms: Ms,
    pub width: u32,
    pub height: u32,
    pub fps: Rational,
    pub codec: String,
    pub container: String,
    pub has_audio: bool,
    pub audio_codec: Option<String>,
    pub sample_rate: Option<u32>,
    /// Display rotation in degrees (0, 90, 180, 270) from container metadata.
    pub rotation: i32,
    /// A still image (png/jpeg/webp): no intrinsic duration, no audio.
    #[serde(default)]
    pub is_still: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MediaKind {
    Video,
    Audio,
    Image,
}

impl MediaInfo {
    pub fn kind(&self) -> MediaKind {
        if self.is_still { MediaKind::Image } else if self.width > 0 { MediaKind::Video } else { MediaKind::Audio }
    }

    /// Width/height as displayed (after rotation metadata is applied).
    pub fn display_size(&self) -> (u32, u32) {
        if self.rotation % 180 != 0 { (self.height, self.width) } else { (self.width, self.height) }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AspectPreset {
    YouTube16x9,
    Shorts9x16,
    Square1x1,
    LinkedIn4x5,
}

impl AspectPreset {
    pub fn dimensions(&self) -> (u32, u32) {
        match self {
            AspectPreset::YouTube16x9 => (1920, 1080),
            AspectPreset::Shorts9x16 => (1080, 1920),
            AspectPreset::Square1x1 => (1080, 1080),
            AspectPreset::LinkedIn4x5 => (1080, 1350),
        }
    }
    pub fn ratio(&self) -> f64 {
        let (w, h) = self.dimensions();
        w as f64 / h as f64
    }
}

/// How the source is framed inside the preset. `scale` >= 1 zooms in; `x`/`y` are the normalized
/// (0..1) centre of the visible region in source space. Default = fit centred.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Crop {
    pub scale: f32,
    pub x: f32,
    pub y: f32,
}

impl Default for Crop {
    fn default() -> Self {
        Crop { scale: 1.0, x: 0.5, y: 0.5 }
    }
}

impl Crop {
    pub fn is_identity(&self) -> bool {
        (self.scale - 1.0).abs() < 1e-4 && (self.x - 0.5).abs() < 1e-4 && (self.y - 0.5).abs() < 1e-4
    }
}

/// Transition applied between a clip and the clip that follows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Transition {
    None,
    CrossDissolve { ms: u32 },
    DipToBlack { ms: u32 },
}

impl Transition {
    pub fn duration_ms(&self) -> Ms {
        match self {
            Transition::None => 0,
            Transition::CrossDissolve { ms } | Transition::DipToBlack { ms } => *ms as Ms,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    pub id: Uuid,
    pub source: PathBuf,
    /// User-given name shown on the timeline; `None` = the source file name.
    #[serde(default)]
    pub name: Option<String>,
    pub media: MediaInfo,
    pub source_start: Ms,
    pub source_end: Ms,
    /// Derived by `timeline::relayout`; never set by hand.
    pub timeline_start: Ms,
    pub fade_in: Ms,
    pub fade_out: Ms,
    pub volume: f32,
    pub muted: bool,
    pub transition_out: Transition,
}

impl Clip {
    pub fn new(source: PathBuf, media: MediaInfo) -> Self {
        Clip {
            id: Uuid::new_v4(),
            source,
            name: None,
            source_end: media.duration_ms,
            media,
            source_start: 0,
            timeline_start: 0,
            fade_in: 0,
            fade_out: 0,
            volume: 1.0,
            muted: false,
            transition_out: Transition::None,
        }
    }
    pub fn duration_ms(&self) -> Ms {
        self.source_end.saturating_sub(self.source_start)
    }
}

/// Where an overlay sits inside the output frame: `scale` = overlay width / output width,
/// `x`/`y` = normalized centre. Default for stills is a small bottom-right badge; video fills the frame.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Placement {
    pub scale: f32,
    pub x: f32,
    pub y: f32,
}

impl Placement {
    pub const FULL: Placement = Placement { scale: 1.0, x: 0.5, y: 0.5 };
    pub const BADGE: Placement = Placement { scale: 0.35, x: 0.85, y: 0.85 };
    pub fn clamped(self) -> Placement {
        Placement { scale: self.scale.clamp(0.05, 1.0), x: self.x.clamp(0.0, 1.0), y: self.y.clamp(0.0, 1.0) }
    }
}

/// A clip on the overlay (V2) track: free-positioned, silent, composited above V1.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OverlayClip {
    pub id: Uuid,
    pub source: PathBuf,
    /// User-given name shown on the timeline; `None` = the source file name.
    #[serde(default)]
    pub name: Option<String>,
    pub media: MediaInfo,
    pub source_start: Ms,
    /// For stills this is simply the on-screen length (source_start stays 0).
    pub source_end: Ms,
    pub timeline_start: Ms,
    pub fade_in: Ms,
    pub fade_out: Ms,
    pub placement: Placement,
    /// Which overlay row (0 = V2, 1 = V3, …). Higher layers composite on top.
    #[serde(default)]
    pub layer: u32,
}

impl OverlayClip {
    pub fn new(source: PathBuf, media: MediaInfo) -> Self {
        let still = media.is_still;
        OverlayClip {
            id: Uuid::new_v4(),
            source,
            name: None,
            source_end: if still { STILL_DEFAULT_MS } else { media.duration_ms },
            media,
            source_start: 0,
            timeline_start: 0,
            fade_in: 0,
            fade_out: 0,
            placement: if still { Placement::BADGE } else { Placement::FULL },
            layer: 0,
        }
    }
    pub fn duration_ms(&self) -> Ms {
        self.source_end.saturating_sub(self.source_start)
    }
    pub fn end_ms(&self) -> Ms {
        self.timeline_start + self.duration_ms()
    }
}

/// A clip on an audio track: free-positioned.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioClip {
    pub id: Uuid,
    pub source: PathBuf,
    /// User-given name shown on the timeline; `None` = the source file name.
    #[serde(default)]
    pub name: Option<String>,
    pub media: MediaInfo,
    pub source_start: Ms,
    pub source_end: Ms,
    pub timeline_start: Ms,
    pub volume: f32,
    pub fade_in: Ms,
    pub fade_out: Ms,
    pub muted: bool,
}

impl AudioClip {
    pub fn new(source: PathBuf, media: MediaInfo) -> Self {
        AudioClip {
            id: Uuid::new_v4(),
            source,
            name: None,
            source_end: media.duration_ms,
            media,
            source_start: 0,
            timeline_start: 0,
            volume: 1.0,
            fade_in: 0,
            fade_out: 0,
            muted: false,
        }
    }
    pub fn duration_ms(&self) -> Ms {
        self.source_end.saturating_sub(self.source_start)
    }
    pub fn end_ms(&self) -> Ms {
        self.timeline_start + self.duration_ms()
    }
}

/// A labelled audio track (Music, SFX, Narration, …) holding free-positioned clips.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioTrack {
    pub id: Uuid,
    pub label: String,
    pub muted: bool,
    /// Track fader, 0–1: multiplies every clip's own volume on export and in the preview.
    #[serde(default = "full")]
    pub volume: f32,
    pub clips: Vec<AudioClip>,
}

impl AudioTrack {
    pub fn new(label: impl Into<String>) -> Self {
        AudioTrack { id: Uuid::new_v4(), label: label.into(), muted: false, volume: 1.0, clips: Vec::new() }
    }
}

/// Media the user has imported but not necessarily placed. Kind comes from `media.kind()`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PoolItem {
    pub id: Uuid,
    pub path: PathBuf,
    pub media: MediaInfo,
}

/// One caption line. Times are in *source* time, so timeline edits never desync captions;
/// `ai::captions::timeline_cues` maps them through the V1 clips.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cue {
    pub id: Uuid,
    pub start: Ms,
    pub end: Ms,
    pub text: String,
}

/// What was said in one source file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Transcript {
    pub source: PathBuf,
    pub cues: Vec<Cue>,
}

/// A span of the timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Range {
    pub start: Ms,
    pub end: Ms,
}

/// A section worth cutting into a short, with the plan to build it. Times are *timeline* time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Highlight {
    pub id: Uuid,
    pub title: String,
    pub reason: String,
    pub start: Ms,
    pub end: Ms,
    /// The parts of `start..end` that make the cut, in order. Never empty.
    pub keep: Vec<Range>,
    pub fade_in: Ms,
    pub fade_out: Ms,
    /// Suggestions the app cannot apply by itself. Display only.
    pub notes: Vec<String>,
}

/// The pre-v2 single music bed. Only ever read, then migrated into an `AudioTrack`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LegacyMusic {
    pub source: PathBuf,
    pub duration_ms: Ms,
    pub timeline_start: Ms,
    pub trim_start: Ms,
    pub trim_end: Ms,
    pub volume: f32,
    pub fade_in: Ms,
    pub fade_out: Ms,
    pub muted: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub version: u32,
    pub id: Uuid,
    pub name: String,
    pub aspect: AspectPreset,
    pub crop: Crop,
    pub clips: Vec<Clip>,
    #[serde(default)]
    pub overlays: Vec<OverlayClip>,
    /// Number of overlay rows shown (V2, V3, …). Always ≥ 1 and ≥ every clip's layer + 1.
    #[serde(default = "one")]
    pub overlay_layers: u32,
    /// Track-level mute for V1: every clip's audio is silenced on export and in the preview.
    #[serde(default)]
    pub video_muted: bool,
    /// V1 track fader, 0–1, on top of each clip's volume.
    #[serde(default = "full")]
    pub video_volume: f32,
    #[serde(default)]
    pub audio_tracks: Vec<AudioTrack>,
    #[serde(default)]
    pub pool: Vec<PoolItem>,
    #[serde(default)]
    pub transcripts: Vec<Transcript>,
    #[serde(default)]
    pub highlights: Vec<Highlight>,
    /// v1 files only; `migrate` folds it into `audio_tracks` and clears it. Never written.
    #[serde(default, skip_serializing)]
    pub music: Option<LegacyMusic>,
    /// Output frame rate. `None` = follow the first clip.
    pub fps: Option<Rational>,
}

impl Project {
    pub fn new(name: impl Into<String>) -> Self {
        Project {
            version: PROJECT_FILE_VERSION,
            id: Uuid::new_v4(),
            name: name.into(),
            aspect: AspectPreset::YouTube16x9,
            crop: Crop::default(),
            clips: Vec::new(),
            overlays: Vec::new(),
            overlay_layers: 1,
            video_muted: false,
            video_volume: 1.0,
            audio_tracks: Vec::new(),
            pool: Vec::new(),
            transcripts: Vec::new(),
            highlights: Vec::new(),
            music: None,
            fps: None,
        }
    }

    /// Total timeline length after transitions overlap.
    pub fn duration_ms(&self) -> Ms {
        match self.clips.last() {
            Some(c) => c.timeline_start + c.duration_ms(),
            None => 0,
        }
    }

    pub fn output_fps(&self) -> Rational {
        self.fps
            .or_else(|| self.clips.first().map(|c| c.media.fps))
            .filter(|r| r.num > 0 && r.den > 0)
            .unwrap_or(Rational { num: 30, den: 1 })
    }

    pub fn clip_index(&self, id: Uuid) -> Option<usize> {
        self.clips.iter().position(|c| c.id == id)
    }
    pub fn overlay_index(&self, id: Uuid) -> Option<usize> {
        self.overlays.iter().position(|c| c.id == id)
    }
    pub fn audio_track_index(&self, id: Uuid) -> Option<usize> {
        self.audio_tracks.iter().position(|t| t.id == id)
    }
    /// (track index, clip index) for an audio clip id, searching every track.
    pub fn audio_clip_index(&self, id: Uuid) -> Option<(usize, usize)> {
        self.audio_tracks.iter().enumerate().find_map(|(ti, t)| t.clips.iter().position(|c| c.id == id).map(|ci| (ti, ci)))
    }

    /// Bring a v1 file up to date: the single music bed becomes one "Music" track.
    pub fn migrate(&mut self) {
        if let Some(m) = self.music.take() {
            let media = MediaInfo {
                duration_ms: m.duration_ms, width: 0, height: 0, fps: Rational { num: 0, den: 1 },
                codec: String::new(), container: String::new(), has_audio: true,
                audio_codec: None, sample_rate: None, rotation: 0, is_still: false,
            };
            let mut clip = AudioClip::new(m.source, media);
            clip.source_start = m.trim_start;
            clip.source_end = m.trim_end;
            clip.timeline_start = m.timeline_start;
            clip.volume = m.volume;
            clip.fade_in = m.fade_in;
            clip.fade_out = m.fade_out;
            clip.muted = m.muted;
            let mut track = AudioTrack::new("Music");
            track.clips.push(clip);
            self.audio_tracks.push(track);
        }
        self.version = PROJECT_FILE_VERSION;
    }
}

impl Default for Project {
    fn default() -> Self {
        Project::new("Untitled")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn media() -> MediaInfo {
        MediaInfo {
            duration_ms: 4000, width: 1920, height: 1080, fps: Rational { num: 30000, den: 1001 },
            codec: "h264".into(), container: "mov,mp4".into(), has_audio: true,
            audio_codec: Some("aac".into()), sample_rate: Some(48000), rotation: 0, is_still: false,
        }
    }

    #[test]
    fn rational_math_and_zero_guards() {
        let r = Rational { num: 30000, den: 1001 };
        assert!((r.as_f64() - 29.97).abs() < 0.01);
        assert!((r.frame_ms() - 33.366).abs() < 0.01);
        assert_eq!(Rational { num: 1, den: 0 }.as_f64(), 0.0);
        assert_eq!(Rational { num: 0, den: 1 }.frame_ms(), 0.0);
        assert_eq!(Rational { num: 25, den: 1 }.frame_ms(), 40.0);
    }

    #[test]
    fn display_size_swaps_on_quarter_turns() {
        let mut m = media();
        assert_eq!(m.display_size(), (1920, 1080));
        for r in [90, 270, -90] {
            m.rotation = r;
            assert_eq!(m.display_size(), (1080, 1920), "rotation {r}");
        }
        m.rotation = 180;
        assert_eq!(m.display_size(), (1920, 1080));
    }

    #[test]
    fn aspect_presets_have_expected_dimensions_and_ratios() {
        assert_eq!(AspectPreset::YouTube16x9.dimensions(), (1920, 1080));
        assert_eq!(AspectPreset::Shorts9x16.dimensions(), (1080, 1920));
        assert_eq!(AspectPreset::Square1x1.dimensions(), (1080, 1080));
        assert_eq!(AspectPreset::LinkedIn4x5.dimensions(), (1080, 1350));
        assert!((AspectPreset::YouTube16x9.ratio() - 16.0 / 9.0).abs() < 1e-9);
        assert!((AspectPreset::Shorts9x16.ratio() - 9.0 / 16.0).abs() < 1e-9);
        assert_eq!(AspectPreset::Square1x1.ratio(), 1.0);
        assert!((AspectPreset::LinkedIn4x5.ratio() - 0.8).abs() < 1e-9);
        // every preset has even dimensions (required by yuv420p)
        for p in [AspectPreset::YouTube16x9, AspectPreset::Shorts9x16, AspectPreset::Square1x1, AspectPreset::LinkedIn4x5] {
            let (w, h) = p.dimensions();
            assert_eq!((w % 2, h % 2), (0, 0));
        }
    }

    #[test]
    fn crop_identity_detection() {
        assert!(Crop::default().is_identity());
        assert_eq!(Crop::default(), Crop { scale: 1.0, x: 0.5, y: 0.5 });
        assert!(Crop { scale: 1.00001, x: 0.5, y: 0.5 }.is_identity());
        assert!(!Crop { scale: 1.2, x: 0.5, y: 0.5 }.is_identity());
        assert!(!Crop { scale: 1.0, x: 0.4, y: 0.5 }.is_identity());
        assert!(!Crop { scale: 1.0, x: 0.5, y: 0.7 }.is_identity());
    }

    #[test]
    fn transition_duration_and_tagged_json() {
        assert_eq!(Transition::None.duration_ms(), 0);
        assert_eq!(Transition::CrossDissolve { ms: 750 }.duration_ms(), 750);
        assert_eq!(Transition::DipToBlack { ms: 300 }.duration_ms(), 300);
        // The frontend relies on the `type` tag shape.
        assert_eq!(serde_json::to_string(&Transition::None).unwrap(), r#"{"type":"None"}"#);
        assert_eq!(serde_json::to_string(&Transition::CrossDissolve { ms: 500 }).unwrap(), r#"{"type":"CrossDissolve","ms":500}"#);
        let t: Transition = serde_json::from_str(r#"{"type":"DipToBlack","ms":200}"#).unwrap();
        assert_eq!(t, Transition::DipToBlack { ms: 200 });
        assert!(serde_json::from_str::<Transition>(r#"{"type":"Wipe","ms":200}"#).is_err());
    }

    #[test]
    fn clip_new_covers_whole_source_and_duration_saturates() {
        let c = Clip::new(PathBuf::from("/a.mp4"), media());
        assert_eq!((c.source_start, c.source_end), (0, 4000));
        assert_eq!(c.duration_ms(), 4000);
        assert_eq!(c.volume, 1.0);
        assert!(!c.muted);
        assert_eq!(c.transition_out, Transition::None);
        assert_eq!((c.fade_in, c.fade_out, c.timeline_start), (0, 0, 0));
        let mut bad = c.clone();
        bad.source_start = 5000;
        assert_eq!(bad.duration_ms(), 0, "never underflows");
        assert_ne!(Clip::new(PathBuf::from("/a.mp4"), media()).id, c.id);
    }

    #[test]
    fn audio_track_and_clip_defaults() {
        let t = AudioTrack::new("SFX");
        assert_eq!(t.label, "SFX");
        assert!(t.clips.is_empty() && !t.muted);
        let mut m = media();
        m.width = 0; m.height = 0;
        let c = AudioClip::new(PathBuf::from("/m.m4a"), m);
        assert_eq!((c.source_start, c.source_end, c.timeline_start), (0, 4000, 0));
        assert_eq!(c.volume, 1.0);
        assert_eq!(c.end_ms(), 4000);
        assert!(!c.muted);
    }

    #[test]
    fn media_kind_and_overlay_defaults() {
        let v = media();
        assert_eq!(v.kind(), MediaKind::Video);
        let mut a = media(); a.width = 0; a.height = 0;
        assert_eq!(a.kind(), MediaKind::Audio);
        let mut i = media(); i.is_still = true; i.has_audio = false;
        assert_eq!(i.kind(), MediaKind::Image);
        let ov = OverlayClip::new(PathBuf::from("/a.mp4"), v);
        assert_eq!((ov.source_start, ov.source_end), (0, 4000));
        assert_eq!(ov.placement, Placement::FULL);
        let os = OverlayClip::new(PathBuf::from("/l.png"), i);
        assert_eq!(os.source_end, STILL_DEFAULT_MS);
        assert_eq!(os.placement, Placement::BADGE);
        assert_eq!(Placement { scale: 9.0, x: -1.0, y: 2.0 }.clamped(), Placement { scale: 1.0, x: 0.0, y: 1.0 });
    }

    #[test]
    fn v1_music_bed_migrates_into_a_music_track() {
        let mut p = Project::new("old");
        p.version = 1;
        p.music = Some(LegacyMusic {
            source: PathBuf::from("/bed.m4a"), duration_ms: 9000, timeline_start: 2500, trim_start: 1000, trim_end: 8000,
            volume: 0.3, fade_in: 500, fade_out: 1000, muted: false,
        });
        p.migrate();
        assert!(p.music.is_none());
        assert_eq!(p.version, PROJECT_FILE_VERSION);
        assert_eq!(p.audio_tracks.len(), 1);
        let t = &p.audio_tracks[0];
        assert_eq!(t.label, "Music");
        let c = &t.clips[0];
        assert_eq!((c.source_start, c.source_end, c.timeline_start), (1000, 8000, 2500));
        assert_eq!((c.fade_in, c.fade_out, c.volume), (500, 1000, 0.3));
        assert_eq!(c.media.duration_ms, 9000);
        assert_eq!(c.media.kind(), MediaKind::Audio);
        // Idempotent and the bed is never written back.
        p.migrate();
        assert_eq!(p.audio_tracks.len(), 1);
        assert!(!serde_json::to_string(&p).unwrap().contains("\"music\""));
    }

    #[test]
    fn v1_json_without_new_fields_still_loads() {
        let j = r#"{"version":1,"id":"6f6f5c1e-0000-4000-8000-000000000000","name":"x","aspect":"Square1x1",
            "crop":{"scale":1.0,"x":0.5,"y":0.5},"clips":[],"music":null,"fps":null}"#;
        let p: Project = serde_json::from_str(j).unwrap();
        assert!(p.overlays.is_empty() && p.audio_tracks.is_empty() && p.pool.is_empty());
        assert!(p.transcripts.is_empty() && p.highlights.is_empty());
        assert_eq!(p.overlay_layers, 1);
        assert!(!p.video_muted);
        assert_eq!(p.video_volume, 1.0);
    }

    #[test]
    fn project_defaults_duration_and_lookup() {
        let p = Project::default();
        assert_eq!(p.name, "Untitled");
        assert_eq!(p.version, PROJECT_FILE_VERSION);
        assert_eq!(p.aspect, AspectPreset::YouTube16x9);
        assert!(p.crop.is_identity());
        assert!(p.clips.is_empty() && p.audio_tracks.is_empty() && p.overlays.is_empty() && p.fps.is_none());
        assert_eq!(p.duration_ms(), 0);
        assert_eq!(p.clip_index(Uuid::new_v4()), None);

        let mut p = Project::new("named");
        let c = Clip::new(PathBuf::from("/a.mp4"), media());
        let id = c.id;
        p.clips.push(c);
        p.clips[0].timeline_start = 1000;
        assert_eq!(p.duration_ms(), 5000, "last clip start + length");
        assert_eq!(p.clip_index(id), Some(0));
    }

    #[test]
    fn output_fps_prefers_explicit_then_first_clip_then_30() {
        let mut p = Project::new("t");
        assert_eq!(p.output_fps(), Rational { num: 30, den: 1 });
        p.clips.push(Clip::new(PathBuf::from("/a.mp4"), media()));
        assert_eq!(p.output_fps(), Rational { num: 30000, den: 1001 });
        p.fps = Some(Rational { num: 60, den: 1 });
        assert_eq!(p.output_fps(), Rational { num: 60, den: 1 });
        // an unusable fps (0) is ignored
        p.fps = Some(Rational { num: 0, den: 1 });
        assert_eq!(p.output_fps(), Rational { num: 30, den: 1 });
        p.fps = None;
        p.clips[0].media.fps = Rational { num: 0, den: 0 };
        assert_eq!(p.output_fps(), Rational { num: 30, den: 1 });
    }

    #[test]
    fn project_json_round_trip_is_lossless() {
        let mut p = Project::new("rt");
        let mut c = Clip::new(PathBuf::from("/a.mp4"), media());
        c.transition_out = Transition::CrossDissolve { ms: 400 };
        c.fade_in = 100;
        p.clips.push(c);
        let mut t = AudioTrack::new("Music");
        let mut am = media(); am.width = 0;
        t.clips.push(AudioClip::new(PathBuf::from("/m.m4a"), am));
        p.audio_tracks.push(t);
        let mut st = media(); st.is_still = true;
        p.overlays.push(OverlayClip::new(PathBuf::from("/l.png"), st));
        p.pool.push(PoolItem { id: Uuid::new_v4(), path: PathBuf::from("/a.mp4"), media: media() });
        p.crop = Crop { scale: 1.5, x: 0.2, y: 0.8 };
        p.transcripts.push(Transcript { source: PathBuf::from("/a.mp4"), cues: vec![Cue { id: Uuid::new_v4(), start: 0, end: 900, text: "hello".into() }] });
        p.highlights.push(Highlight {
            id: Uuid::new_v4(), title: "Hook".into(), reason: "r".into(), start: 0, end: 3000,
            keep: vec![Range { start: 0, end: 3000 }], fade_in: 0, fade_out: 500, notes: vec!["n".into()],
        });
        let json = serde_json::to_string(&p).unwrap();
        let back: Project = serde_json::from_str(&json).unwrap();
        assert_eq!(back, p);
        assert!(json.contains(r#""aspect":"YouTube16x9""#));
    }
}
