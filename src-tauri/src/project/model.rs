//! Domain model. Rust is the source of truth; the frontend mirrors these types in `src/types/project.ts`.
//! All times are integer milliseconds (`Ms`). Editing only mutates this data — it never touches media.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

pub type Ms = u64;

pub const PROJECT_FILE_VERSION: u32 = 1;

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
}

impl MediaInfo {
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioTrack {
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

impl AudioTrack {
    pub fn new(source: PathBuf, duration_ms: Ms) -> Self {
        AudioTrack {
            source,
            duration_ms,
            timeline_start: 0,
            trim_start: 0,
            trim_end: duration_ms,
            volume: 0.5,
            fade_in: 0,
            fade_out: 0,
            muted: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub version: u32,
    pub id: Uuid,
    pub name: String,
    pub aspect: AspectPreset,
    pub crop: Crop,
    pub clips: Vec<Clip>,
    pub music: Option<AudioTrack>,
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
            audio_codec: Some("aac".into()), sample_rate: Some(48000), rotation: 0,
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
    fn audio_track_defaults() {
        let t = AudioTrack::new(PathBuf::from("/m.m4a"), 9000);
        assert_eq!((t.trim_start, t.trim_end, t.duration_ms), (0, 9000, 9000));
        assert_eq!(t.volume, 0.5, "music defaults to a bed level");
        assert_eq!(t.timeline_start, 0);
        assert!(!t.muted);
    }

    #[test]
    fn project_defaults_duration_and_lookup() {
        let p = Project::default();
        assert_eq!(p.name, "Untitled");
        assert_eq!(p.version, PROJECT_FILE_VERSION);
        assert_eq!(p.aspect, AspectPreset::YouTube16x9);
        assert!(p.crop.is_identity());
        assert!(p.clips.is_empty() && p.music.is_none() && p.fps.is_none());
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
        p.music = Some(AudioTrack::new(PathBuf::from("/m.m4a"), 3000));
        p.crop = Crop { scale: 1.5, x: 0.2, y: 0.8 };
        let json = serde_json::to_string(&p).unwrap();
        let back: Project = serde_json::from_str(&json).unwrap();
        assert_eq!(back, p);
        assert!(json.contains(r#""aspect":"YouTube16x9""#));
    }
}
