//! Speech to text: ffmpeg extracts 16 kHz mono audio, `whisper-cli` writes JSON next to the other
//! caches for that source. A finished transcript is reused until the file changes.

use super::{run_tool, Cancel};
use crate::error::{Error, Result};
use crate::project::{Cue, Ms};
use crate::render::ffmpeg;
use std::path::Path;
use uuid::Uuid;

pub const TRANSCRIPT_FILE: &str = "transcript.json";
/// Share of a source's progress spent extracting audio; the rest is whisper.
const EXTRACT_SHARE: f32 = 0.1;

/// Read whisper-cli's `-oj` output. Non-speech markers such as `[BLANK_AUDIO]`, `(music)` and
/// `♪ ♪` (anything without a letter or digit) are skipped.
pub fn parse_whisper_json(json: &str) -> Result<Vec<Cue>> {
    let v: serde_json::Value = serde_json::from_str(json)?;
    let segs = v["transcription"].as_array().ok_or_else(|| Error::Tool("transcript has no `transcription` list".into()))?;
    let mut cues = Vec::new();
    for s in segs {
        let text = s["text"].as_str().unwrap_or("").trim();
        let marker = (text.starts_with('[') && text.ends_with(']')) || (text.starts_with('(') && text.ends_with(')'))
            || !text.chars().any(char::is_alphanumeric);
        let (Some(start), Some(end)) = (s["offsets"]["from"].as_u64(), s["offsets"]["to"].as_u64()) else { continue };
        if text.is_empty() || marker || end <= start {
            continue;
        }
        cues.push(Cue { id: Uuid::new_v4(), start, end, text: text.to_string() });
    }
    Ok(cues)
}

/// `whisper_print_progress_callback: progress =  45%` → 0.45
pub fn parse_progress(line: &str) -> Option<f32> {
    let rest = line.split("progress =").nth(1)?;
    let n: f32 = rest.trim().trim_end_matches('%').trim().parse().ok()?;
    Some((n / 100.0).clamp(0.0, 1.0))
}

fn strs(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| s.to_string()).collect()
}

/// Transcribe one source. `on_progress` receives 0.0..1.0 for this source.
pub async fn transcribe(
    source: &Path,
    duration_ms: Ms,
    whisper: &Path,
    model: &Path,
    mut on_progress: impl FnMut(f32) + Send,
    cancel: Cancel,
) -> Result<Vec<Cue>> {
    let dir = crate::cache::media_cache_dir(source)?;
    let done = dir.join(TRANSCRIPT_FILE);
    if let Ok(json) = std::fs::read_to_string(&done) {
        if let Ok(cues) = parse_whisper_json(&json) {
            on_progress(1.0);
            return Ok(cues);
        }
    }
    let wav = dir.join("audio16k.wav");
    let part = dir.join("transcript.part");
    let part_json = dir.join("transcript.part.json");
    let cleanup = || {
        let _ = std::fs::remove_file(&wav);
        let _ = std::fs::remove_file(&part_json);
    };

    let mut args = strs(&["-y", "-i"]);
    args.push(source.to_string_lossy().to_string());
    args.extend(strs(&["-vn", "-ac", "1", "-ar", "16000", "-c:a", "pcm_s16le"]));
    args.push(wav.to_string_lossy().to_string());
    if let Err(e) = ffmpeg::run_with_progress(&args, duration_ms, |f| on_progress(f * EXTRACT_SHARE), Some(cancel.oneshot())).await {
        cleanup();
        return Err(e);
    }

    let english_only = model.file_name().map(|n| n.to_string_lossy().contains(".en")).unwrap_or(false);
    let args = vec![
        "-m".to_string(), model.to_string_lossy().to_string(),
        "-f".to_string(), wav.to_string_lossy().to_string(),
        "-l".to_string(), if english_only { "en" } else { "auto" }.to_string(),
        "-oj".to_string(),
        "-of".to_string(), part.to_string_lossy().to_string(),
        "-pp".to_string(),
    ];
    let res = run_tool(
        whisper, &args, None, None,
        |l| if let Some(f) = parse_progress(l) { on_progress(EXTRACT_SHARE + f * (1.0 - EXTRACT_SHARE)) },
        cancel,
    )
    .await;
    let cues = res.and_then(|_| {
        let json = String::from_utf8_lossy(&std::fs::read(&part_json)?).to_string();
        let cues = parse_whisper_json(&json)?;
        std::fs::rename(&part_json, &done)?;
        Ok(cues)
    });
    cleanup();
    on_progress(1.0);
    cues
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::test_support::script;
    use crate::test_util::fixture;

    const SAMPLE: &str = r#"{"systeminfo":"x","result":{"language":"en"},"transcription":[
        {"timestamps":{"from":"00:00:00,000","to":"00:00:02,500"},"offsets":{"from":0,"to":2500},"text":" So here is the thing."},
        {"timestamps":{"from":"00:00:02,500","to":"00:00:04,000"},"offsets":{"from":2500,"to":4000},"text":" [BLANK_AUDIO]"},
        {"offsets":{"from":4000,"to":4000},"text":" zero length"},
        {"offsets":{"from":4000,"to":6000},"text":"   "},
        {"offsets":{"from":6000,"to":9100},"text":" (music)"},
        {"offsets":{"from":6000,"to":9100},"text":" ♪ ♪"},
        {"offsets":{"from":6000,"to":9100},"text":" ..."},
        {"text":" no offsets"},
        {"offsets":{"from":9100,"to":12000},"text":" Nobody tells you this."}]}"#;

    #[test]
    fn whisper_json_becomes_cues_without_markers_or_empties() {
        let cues = parse_whisper_json(SAMPLE).unwrap();
        let got: Vec<(Ms, Ms, &str)> = cues.iter().map(|c| (c.start, c.end, c.text.as_str())).collect();
        assert_eq!(got, vec![(0, 2500, "So here is the thing."), (9100, 12000, "Nobody tells you this.")]);
        assert_ne!(cues[0].id, cues[1].id);
        assert!(matches!(parse_whisper_json("{}"), Err(Error::Tool(_))));
        assert!(matches!(parse_whisper_json("not json"), Err(Error::Json(_))));
        assert!(parse_whisper_json(r#"{"transcription":[]}"#).unwrap().is_empty());
    }

    #[test]
    fn progress_lines_are_parsed() {
        assert_eq!(parse_progress("whisper_print_progress_callback: progress =  45%"), Some(0.45));
        assert_eq!(parse_progress("progress = 100%"), Some(1.0));
        assert_eq!(parse_progress("progress = 250%"), Some(1.0));
        assert_eq!(parse_progress("whisper_init_from_file: loading model"), None);
        assert_eq!(parse_progress("progress = soon"), None);
    }

    /// A copy of a fixture, so each test has its own cache directory.
    fn own_source(dir: &Path) -> std::path::PathBuf {
        let src = dir.join("talk.mp4");
        std::fs::copy(fixture("clip_a_720p.mp4"), &src).unwrap();
        src
    }

    /// Stands in for whisper-cli: checks the wav exists, reports progress, writes `<-of>.json`.
    fn fake_whisper(dir: &Path) -> std::path::PathBuf {
        let sample = dir.join("sample.json");
        std::fs::write(&sample, SAMPLE).unwrap();
        script(dir, "whisper-cli", &format!(
            "while [ $# -gt 0 ]; do case \"$1\" in -f) wav=\"$2\"; shift;; -of) out=\"$2\"; shift;; esac; shift; done\n\
             [ -s \"$wav\" ] || {{ echo 'no wav' >&2; exit 2; }}\n\
             echo 'whisper_print_progress_callback: progress =  50%' >&2\n\
             cp '{}' \"$out.json\"", sample.display()))
    }

    #[tokio::test]
    async fn transcribes_then_reuses_the_cached_transcript() {
        let dir = tempfile::tempdir().unwrap();
        let src = own_source(dir.path());
        let whisper = fake_whisper(dir.path());
        let model = dir.path().join("ggml-base.en.bin");
        let mut seen = Vec::new();
        let cues = transcribe(&src, 5000, &whisper, &model, |f| seen.push(f), Cancel::never()).await.unwrap();
        assert_eq!(cues.len(), 2);
        assert!(seen.windows(2).all(|w| w[0] <= w[1]), "monotonic: {seen:?}");
        assert!(seen.contains(&0.55), "whisper's 50 % lands after the extract share: {seen:?}");
        assert_eq!(*seen.last().unwrap(), 1.0);
        let cache = crate::cache::media_cache_dir(&src).unwrap();
        assert!(cache.join(TRANSCRIPT_FILE).is_file());
        assert!(!cache.join("audio16k.wav").exists(), "the wav is removed");
        assert!(!cache.join("transcript.part.json").exists());

        // Second run never starts whisper.
        let broken = script(dir.path(), "broken", "exit 9");
        let again = transcribe(&src, 5000, &broken, &model, |_| {}, Cancel::never()).await.unwrap();
        assert_eq!(again.len(), 2);
        std::fs::remove_dir_all(cache).unwrap();
    }

    #[tokio::test]
    async fn whisper_failure_and_missing_source_leave_nothing_behind() {
        let dir = tempfile::tempdir().unwrap();
        let src = own_source(dir.path());
        let model = dir.path().join("m.bin");
        let bad = script(dir.path(), "whisper-cli", "echo 'failed to load model' >&2\nexit 1");
        let err = transcribe(&src, 5000, &bad, &model, |_| {}, Cancel::never()).await.unwrap_err();
        assert!(matches!(&err, Error::Tool(m) if m.contains("failed to load model")), "{err:?}");
        let cache = crate::cache::media_cache_dir(&src).unwrap();
        assert!(!cache.join(TRANSCRIPT_FILE).exists() && !cache.join("audio16k.wav").exists());
        std::fs::remove_dir_all(cache).unwrap();
        assert!(transcribe(&dir.path().join("gone.mp4"), 1000, &bad, &model, |_| {}, Cancel::never()).await.is_err());
    }

    #[tokio::test]
    async fn cancel_stops_whisper() {
        let dir = tempfile::tempdir().unwrap();
        let src = own_source(dir.path());
        let slow = script(dir.path(), "whisper-cli", "sleep 60");
        let (tx, rx) = tokio::sync::oneshot::channel();
        let cancel = Cancel::from_oneshot(rx);
        let (s2, m) = (src.clone(), dir.path().join("m.bin"));
        let handle = tokio::spawn(async move { transcribe(&s2, 5000, &slow, &m, |_| {}, cancel).await });
        std::thread::sleep(std::time::Duration::from_millis(1500));
        tx.send(()).unwrap();
        assert!(matches!(handle.await.unwrap(), Err(Error::Cancelled)));
        let cache = crate::cache::media_cache_dir(&src).unwrap();
        assert!(!cache.join(TRANSCRIPT_FILE).exists());
        std::fs::remove_dir_all(cache).unwrap();
    }
}
