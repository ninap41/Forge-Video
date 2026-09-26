pub mod thumbs;
pub mod waveform;

use crate::error::Result;
use std::path::{Path, PathBuf};

pub fn cache_root() -> PathBuf {
    dirs::cache_dir().unwrap_or_else(std::env::temp_dir).join("ForgeVideo")
}

/// Cache key: content identity by path + size + mtime (cheap, no hashing of the file body).
pub fn media_key(path: &Path) -> Result<String> {
    let md = std::fs::metadata(path)?;
    let mtime = md.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
    let mut h = blake3::Hasher::new();
    h.update(path.to_string_lossy().as_bytes());
    h.update(&md.len().to_le_bytes());
    h.update(&mtime.to_le_bytes());
    Ok(h.finalize().to_hex()[..24].to_string())
}

pub fn media_cache_dir(path: &Path) -> Result<PathBuf> {
    let dir = cache_root().join(media_key(path)?);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_root_is_under_forge_video() {
        let r = cache_root();
        assert_eq!(r.file_name().unwrap(), "ForgeVideo");
        assert!(r.is_absolute());
    }

    #[test]
    fn media_key_is_stable_and_sensitive_to_path_and_size() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.mp4");
        let b = dir.path().join("b.mp4");
        std::fs::write(&a, b"aaaa").unwrap();
        std::fs::write(&b, b"aaaa").unwrap();
        let ka = media_key(&a).unwrap();
        assert_eq!(ka.len(), 24);
        assert!(ka.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(ka, media_key(&a).unwrap(), "deterministic");
        assert_ne!(ka, media_key(&b).unwrap(), "same bytes, different path");
        std::fs::write(&a, b"aaaaaaaa").unwrap();
        assert_ne!(ka, media_key(&a).unwrap(), "size change invalidates");
        assert!(matches!(media_key(&dir.path().join("missing.mp4")), Err(crate::error::Error::Io(_))));
    }

    #[test]
    fn media_cache_dir_is_created_under_the_root() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.mp4");
        std::fs::write(&a, b"x").unwrap();
        let d = media_cache_dir(&a).unwrap();
        assert!(d.is_dir());
        assert!(d.starts_with(cache_root()));
        assert_eq!(d.file_name().unwrap().to_str().unwrap(), media_key(&a).unwrap());
        std::fs::remove_dir_all(&d).unwrap();
    }
}
