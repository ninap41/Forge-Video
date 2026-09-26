//! Project file = pretty JSON. Media paths inside the project folder are stored relative so the
//! folder can move; anything outside stays absolute.

use super::model::Project;
use crate::error::Result;
use std::path::{Path, PathBuf};

fn relativize(path: &Path, base: &Path) -> PathBuf {
    path.strip_prefix(base).map(Path::to_path_buf).unwrap_or_else(|_| path.to_path_buf())
}

fn absolutize(path: &Path, base: &Path) -> PathBuf {
    if path.is_absolute() { path.to_path_buf() } else { base.join(path) }
}

pub fn save(project: &Project, file: &Path) -> Result<()> {
    let base = file.parent().unwrap_or_else(|| Path::new("/"));
    let mut p = project.clone();
    for c in &mut p.clips {
        c.source = relativize(&c.source, base);
    }
    if let Some(m) = &mut p.music {
        m.source = relativize(&m.source, base);
    }
    let json = serde_json::to_string_pretty(&p)?;
    std::fs::write(file, json)?;
    Ok(())
}

pub fn load(file: &Path) -> Result<Project> {
    let base = file.parent().unwrap_or_else(|| Path::new("/"));
    let mut p: Project = serde_json::from_str(&std::fs::read_to_string(file)?)?;
    for c in &mut p.clips {
        c.source = absolutize(&c.source, base);
    }
    if let Some(m) = &mut p.music {
        m.source = absolutize(&m.source, base);
    }
    crate::timeline::relayout(&mut p);
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::model::*;

    fn dummy_media() -> MediaInfo {
        MediaInfo {
            duration_ms: 5000, width: 1280, height: 720, fps: Rational { num: 30, den: 1 },
            codec: "h264".into(), container: "mov,mp4".into(), has_audio: true,
            audio_codec: Some("aac".into()), sample_rate: Some(48000), rotation: 0,
        }
    }

    #[test]
    fn round_trips_with_relative_paths() {
        let dir = tempfile::tempdir().unwrap();
        let inside = dir.path().join("media/a.mp4");
        let mut p = Project::new("t");
        p.clips.push(Clip::new(inside.clone(), dummy_media()));
        p.clips.push(Clip::new(PathBuf::from("/elsewhere/b.mp4"), dummy_media()));
        let file = dir.path().join("project.json");
        save(&p, &file).unwrap();
        let raw = std::fs::read_to_string(&file).unwrap();
        assert!(raw.contains("\"media/a.mp4\""));
        assert!(raw.contains("/elsewhere/b.mp4"));
        let loaded = load(&file).unwrap();
        assert_eq!(loaded.clips[0].source, inside);
        assert_eq!(loaded.clips[1].source, PathBuf::from("/elsewhere/b.mp4"));
        assert_eq!(loaded.clips[1].timeline_start, 5000);
    }
}

#[cfg(test)]
mod more_tests {
    use super::*;
    use crate::project::model::*;

    fn media() -> MediaInfo {
        MediaInfo {
            duration_ms: 5000, width: 1280, height: 720, fps: Rational { num: 30, den: 1 },
            codec: "h264".into(), container: "mov,mp4".into(), has_audio: true,
            audio_codec: Some("aac".into()), sample_rate: Some(48000), rotation: 0,
        }
    }

    #[test]
    fn music_path_is_relativised_too() {
        let dir = tempfile::tempdir().unwrap();
        let inside = dir.path().join("bed.m4a");
        let mut p = Project::new("m");
        p.music = Some(AudioTrack::new(inside.clone(), 8000));
        let file = dir.path().join("p.forgevideo");
        save(&p, &file).unwrap();
        let raw = std::fs::read_to_string(&file).unwrap();
        assert!(raw.contains("\"bed.m4a\""), "{raw}");
        assert!(!raw.contains(dir.path().to_str().unwrap()));
        assert_eq!(load(&file).unwrap().music.unwrap().source, inside);
    }

    #[test]
    fn load_relayouts_and_clamps_stale_transitions() {
        // Hand-written file with wrong timeline_start values and an oversized transition.
        let dir = tempfile::tempdir().unwrap();
        let mut p = Project::new("stale");
        let mut a = Clip::new(PathBuf::from("/a.mp4"), media());
        a.timeline_start = 999;
        a.transition_out = Transition::CrossDissolve { ms: 60_000 };
        let mut b = Clip::new(PathBuf::from("/b.mp4"), media());
        b.timeline_start = 1;
        b.transition_out = Transition::DipToBlack { ms: 100 };
        p.clips = vec![a, b];
        let file = dir.path().join("p.json");
        std::fs::write(&file, serde_json::to_string(&p).unwrap()).unwrap();
        let l = load(&file).unwrap();
        assert_eq!(l.clips[0].timeline_start, 0);
        assert_eq!(l.clips[0].transition_out, Transition::CrossDissolve { ms: 2500 });
        assert_eq!(l.clips[1].timeline_start, 2500);
        assert_eq!(l.clips[1].transition_out, Transition::None, "last clip");
    }

    #[test]
    fn load_errors_are_typed() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(load(&dir.path().join("nope.json")), Err(crate::error::Error::Io(_))));
        let bad = dir.path().join("bad.json");
        std::fs::write(&bad, "{ not json").unwrap();
        assert!(matches!(load(&bad), Err(crate::error::Error::Json(_))));
        std::fs::write(&bad, r#"{"version":1}"#).unwrap();
        assert!(matches!(load(&bad), Err(crate::error::Error::Json(_))), "missing fields");
    }

    #[test]
    fn save_into_missing_directory_fails_and_leaves_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("missing/sub/p.json");
        assert!(matches!(save(&Project::new("x"), &file), Err(crate::error::Error::Io(_))));
        assert!(!file.exists());
    }

    #[test]
    fn saved_file_is_pretty_json_with_version() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("p.json");
        save(&Project::new("pretty"), &file).unwrap();
        let raw = std::fs::read_to_string(&file).unwrap();
        assert!(raw.starts_with("{\n"));
        assert!(raw.contains(&format!("\"version\": {PROJECT_FILE_VERSION}")));
        assert!(raw.contains("\"name\": \"pretty\""));
    }

    #[test]
    fn empty_project_round_trips_and_does_not_change_the_original() {
        let dir = tempfile::tempdir().unwrap();
        let mut p = Project::new("e");
        p.clips.push(Clip::new(dir.path().join("a.mp4"), media()));
        let before = p.clone();
        let file = dir.path().join("p.json");
        save(&p, &file).unwrap();
        assert_eq!(p, before, "save works on a copy");
        let l = load(&file).unwrap();
        assert_eq!(l.clips[0].source, dir.path().join("a.mp4"));
        assert_eq!(l.id, p.id);
    }
}
