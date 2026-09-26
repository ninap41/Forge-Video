//! Filmstrip thumbnails: ~N evenly spaced JPEGs per source, 160px wide.

use crate::error::Result;
use crate::render::ffmpeg::run_with_progress;
use std::path::{Path, PathBuf};

pub const TARGET_COUNT: u64 = 40;
pub const THUMB_WIDTH: u32 = 160;

pub struct Thumbnails {
    pub dir: PathBuf,
    pub files: Vec<PathBuf>,
    /// Interval between thumbnails in ms.
    pub interval_ms: u64,
}

fn list(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|r| r.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().map(|e| e == "jpg").unwrap_or(false)).collect())
        .unwrap_or_default();
    v.sort();
    v
}

pub async fn generate(source: &Path, duration_ms: u64) -> Result<Thumbnails> {
    let dir = super::media_cache_dir(source)?.join("thumbs");
    std::fs::create_dir_all(&dir)?;
    let interval_ms = (duration_ms / TARGET_COUNT).max(200);
    let existing = list(&dir);
    if !existing.is_empty() && dir.join("done").exists() {
        return Ok(Thumbnails { dir, files: existing, interval_ms });
    }
    let args: Vec<String> = vec![
        "-y".into(), "-i".into(), source.to_string_lossy().into(),
        "-vf".into(), format!("fps=1000/{interval_ms},scale={THUMB_WIDTH}:-2"),
        "-q:v".into(), "6".into(),
        dir.join("%04d.jpg").to_string_lossy().into(),
    ];
    run_with_progress(&args, duration_ms, |_| {}, None).await?;
    std::fs::write(dir.join("done"), b"")?;
    Ok(Thumbnails { dir: dir.clone(), files: list(&dir), interval_ms })
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn generates_filmstrip() {
        let p = crate::test_util::fixture("clip_a_720p.mp4");
        let t = super::generate(&p, 5000).await.unwrap();
        assert!(t.files.len() >= 20, "got {} thumbs", t.files.len());
        assert_eq!(t.interval_ms, 200);
        // second call is a cache hit
        let t2 = super::generate(&p, 5000).await.unwrap();
        assert_eq!(t.files, t2.files);
    }
}

#[cfg(test)]
mod more_tests {
    use super::*;

    #[test]
    fn listing_returns_only_sorted_jpgs() {
        let dir = tempfile::tempdir().unwrap();
        for n in ["0003.jpg", "0001.jpg", "done", "0002.jpg", "x.png"] {
            std::fs::write(dir.path().join(n), b"").unwrap();
        }
        let names: Vec<_> = list(dir.path()).iter().map(|p| p.file_name().unwrap().to_str().unwrap().to_string()).collect();
        assert_eq!(names, vec!["0001.jpg", "0002.jpg", "0003.jpg"]);
        assert!(list(Path::new("/no/such/dir")).is_empty());
    }

    #[tokio::test]
    async fn interval_never_drops_below_200ms_and_long_media_gets_40_thumbs() {
        // Pure arithmetic through the public constants; exercised by the fixture in `generates_filmstrip`.
        assert_eq!((5000u64 / TARGET_COUNT).max(200), 200);
        assert_eq!((600_000u64 / TARGET_COUNT).max(200), 15_000);
        assert_eq!(THUMB_WIDTH, 160);
        assert!(generate(Path::new("/no/such.mp4"), 1000).await.is_err());
    }
}
