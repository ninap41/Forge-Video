//! Waveform peaks: decode to mono 8 kHz s16le, take the peak of every 10 ms bucket, store as u8.

use crate::error::Result;
use crate::render::ffmpeg::run_capture_stdout;
use std::path::Path;

pub const SAMPLE_RATE: u32 = 8000;
pub const BUCKET_MS: u32 = 10;

pub fn peaks_from_pcm(pcm: &[u8]) -> Vec<u8> {
    let per_bucket = (SAMPLE_RATE * BUCKET_MS / 1000) as usize;
    pcm.chunks_exact(2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]).unsigned_abs())
        .collect::<Vec<u16>>()
        .chunks(per_bucket)
        .map(|c| (c.iter().copied().max().unwrap_or(0) >> 7).min(255) as u8)
        .collect()
}

/// Returns one byte (0..255) per 10 ms bucket.
pub async fn generate(source: &Path) -> Result<Vec<u8>> {
    let file = super::media_cache_dir(source)?.join("waveform.bin");
    if let Ok(bytes) = std::fs::read(&file) {
        if !bytes.is_empty() {
            return Ok(bytes);
        }
    }
    let args: Vec<String> = vec![
        "-i".into(), source.to_string_lossy().into(),
        "-vn".into(), "-ac".into(), "1".into(), "-ar".into(), SAMPLE_RATE.to_string(),
        "-f".into(), "s16le".into(), "-".into(),
    ];
    let pcm = run_capture_stdout(&args).await?;
    let peaks = peaks_from_pcm(&pcm);
    std::fs::write(&file, &peaks)?;
    Ok(peaks)
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn waveform_of_sine_is_loud_everywhere() {
        let p = crate::test_util::fixture("clip_a_720p.mp4");
        let w = super::generate(&p).await.unwrap();
        assert!((480..=520).contains(&w.len()), "buckets {}", w.len());
        let avg = w.iter().map(|&b| b as u32).sum::<u32>() / w.len() as u32;
        assert!(avg > 20, "avg {avg}");
    }
}

#[cfg(test)]
mod peak_tests {
    use super::*;

    fn pcm(samples: &[i16]) -> Vec<u8> {
        samples.iter().flat_map(|s| s.to_le_bytes()).collect()
    }

    #[test]
    fn bucket_geometry() {
        assert_eq!(SAMPLE_RATE * BUCKET_MS / 1000, 80, "80 samples per 10 ms bucket");
        assert!(peaks_from_pcm(&[]).is_empty());
        assert_eq!(peaks_from_pcm(&pcm(&[0; 80])).len(), 1);
        assert_eq!(peaks_from_pcm(&pcm(&[0; 81])).len(), 2, "partial trailing bucket still counts");
        assert_eq!(peaks_from_pcm(&pcm(&[0; 800])).len(), 10);
    }

    #[test]
    fn peaks_scale_to_u8_and_take_the_absolute_max() {
        assert_eq!(peaks_from_pcm(&pcm(&[0; 80])), vec![0]);
        assert_eq!(peaks_from_pcm(&pcm(&[i16::MAX; 80])), vec![255]);
        assert_eq!(peaks_from_pcm(&pcm(&[i16::MIN; 80])), vec![255], "negative full scale is loud too");
        let mut quiet = vec![0i16; 80];
        quiet[17] = -12_800; // one spike, |x| >> 7 = 100
        assert_eq!(peaks_from_pcm(&pcm(&quiet)), vec![100]);
        let mut two = vec![100i16; 80];
        two.extend([i16::MAX; 80]);
        assert_eq!(peaks_from_pcm(&pcm(&two)), vec![0, 255]);
    }

    #[test]
    fn odd_trailing_byte_is_ignored() {
        let mut b = pcm(&[i16::MAX; 80]);
        b.push(0xFF);
        assert_eq!(peaks_from_pcm(&b), vec![255]);
    }

    #[tokio::test]
    async fn waveform_is_cached_on_disk_and_errors_for_missing_files() {
        let p = crate::test_util::fixture("music.m4a");
        let w = super::generate(&p).await.unwrap();
        let file = crate::cache::media_cache_dir(&p).unwrap().join("waveform.bin");
        assert_eq!(std::fs::read(&file).unwrap(), w);
        assert_eq!(super::generate(&p).await.unwrap(), w, "second call reads the cache");
        assert!(super::generate(std::path::Path::new("/no/such.m4a")).await.is_err());
    }
}
