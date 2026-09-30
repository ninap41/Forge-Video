//! Locates and runs ffmpeg/ffprobe. Progress is parsed from `-progress pipe:1` output.

use crate::error::{Error, Result};
use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::oneshot;

/// Directories searched before PATH. Homebrew paths are not on PATH inside a .app bundle launched from Finder.
pub const BIN_DIRS: [&str; 2] = ["/opt/homebrew/bin", "/usr/local/bin"];

/// Locate a CLI: the `env` override wins, then `BIN_DIRS`, then `extra_dirs`, then PATH.
pub fn find_in(name: &str, env: &str, extra_dirs: &[PathBuf]) -> Result<PathBuf> {
    if let Ok(p) = std::env::var(env) {
        return Ok(PathBuf::from(p));
    }
    for dir in BIN_DIRS.iter().map(PathBuf::from).chain(extra_dirs.iter().cloned()) {
        let p = dir.join(name);
        if p.is_file() {
            return Ok(p);
        }
    }
    which::which(name).map_err(|e| Error::BinaryNotFound(format!("{name}: {e}")))
}

fn find(name: &str) -> Result<PathBuf> {
    find_in(name, &format!("FORGE_{}", name.to_uppercase()), &[])
}

pub fn ffmpeg_bin() -> Result<PathBuf> {
    find("ffmpeg")
}
pub fn ffprobe_bin() -> Result<PathBuf> {
    find("ffprobe")
}

/// Run ffmpeg with the given args (without the binary). `total_ms` sizes the progress fraction.
/// `on_progress` receives 0.0..1.0. Dropping/sending on `cancel` kills the process.
pub async fn run_with_progress<F>(
    args: &[String],
    total_ms: u64,
    mut on_progress: F,
    mut cancel: Option<oneshot::Receiver<()>>,
) -> Result<()>
where
    F: FnMut(f32) + Send,
{
    let mut cmd = Command::new(ffmpeg_bin()?);
    cmd.args(["-hide_banner", "-nostdin", "-loglevel", "error", "-progress", "pipe:1", "-nostats"])
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    log::debug!("ffmpeg {}", args.join(" "));
    let mut child = cmd.spawn()?;
    let stdout = child.stdout.take().expect("stdout piped");
    let stderr = child.stderr.take().expect("stderr piped");
    let mut lines = BufReader::new(stdout).lines();
    let stderr_task = tokio::spawn(async move {
        let mut s = String::new();
        let mut r = BufReader::new(stderr).lines();
        while let Ok(Some(l)) = r.next_line().await {
            s.push_str(&l);
            s.push('\n');
        }
        s
    });

    loop {
        tokio::select! {
            line = lines.next_line() => {
                match line? {
                    Some(l) => {
                        if let Some(v) = l.strip_prefix("out_time_us=").or_else(|| l.strip_prefix("out_time_ms=")) {
                            // ffmpeg reports both keys in microseconds.
                            if let Ok(us) = v.trim().parse::<i64>() {
                                if total_ms > 0 && us >= 0 {
                                    on_progress(((us as f64 / 1000.0) / total_ms as f64).min(1.0) as f32);
                                }
                            }
                        }
                    }
                    None => break,
                }
            }
            _ = async { match cancel.as_mut() { Some(c) => { let _ = c.await; } None => std::future::pending::<()>().await } } => {
                let _ = child.kill().await;
                return Err(Error::Cancelled);
            }
        }
    }
    let status = child.wait().await?;
    let err = stderr_task.await.unwrap_or_default();
    if !status.success() {
        return Err(Error::Export(format!("ffmpeg exited with {status}: {}", err.trim())));
    }
    on_progress(1.0);
    Ok(())
}

/// Run ffmpeg to completion, returning stdout bytes (used for raw PCM extraction).
pub async fn run_capture_stdout(args: &[String]) -> Result<Vec<u8>> {
    let out = Command::new(ffmpeg_bin()?)
        .args(["-hide_banner", "-nostdin", "-loglevel", "error"])
        .args(args)
        .stdin(Stdio::null())
        .output()
        .await?;
    if !out.status.success() {
        return Err(Error::Media(String::from_utf8_lossy(&out.stderr).trim().to_string()));
    }
    Ok(out.stdout)
}

pub fn ms_to_secs(ms: u64) -> String {
    format!("{}.{:03}", ms / 1000, ms % 1000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ms_to_secs_always_has_three_decimals() {
        assert_eq!(ms_to_secs(0), "0.000");
        assert_eq!(ms_to_secs(5), "0.005");
        assert_eq!(ms_to_secs(1000), "1.000");
        assert_eq!(ms_to_secs(61_234), "61.234");
        assert_eq!(ms_to_secs(3_600_000), "3600.000");
    }

    #[test]
    fn binaries_are_found_on_this_machine() {
        // These tests already depend on a local ffmpeg for exports/probes.
        assert!(ffmpeg_bin().unwrap().is_file() || ffmpeg_bin().is_ok());
        assert!(ffprobe_bin().is_ok());
        assert!(ffmpeg_bin().unwrap().file_name().unwrap() == "ffmpeg");
    }

    #[tokio::test]
    async fn progress_reaches_one_on_a_synthetic_render() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("t.mp4");
        let args: Vec<String> = ["-y", "-f", "lavfi", "-i", "testsrc=size=64x64:rate=30:duration=1", "-pix_fmt", "yuv420p"]
            .iter().map(|s| s.to_string()).chain([out.to_string_lossy().to_string()]).collect();
        let mut seen = Vec::new();
        run_with_progress(&args, 1000, |f| seen.push(f), None).await.unwrap();
        assert_eq!(*seen.last().unwrap(), 1.0);
        assert!(seen.iter().all(|f| (0.0..=1.0).contains(f)));
        assert!(seen.windows(2).all(|w| w[0] <= w[1]), "monotonic: {seen:?}");
        assert!(out.metadata().unwrap().len() > 0);
    }

    #[tokio::test]
    async fn zero_total_still_reports_completion_without_dividing() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("t.mp4");
        let args: Vec<String> = ["-y", "-f", "lavfi", "-i", "testsrc=size=64x64:rate=30:duration=0.2", "-pix_fmt", "yuv420p"]
            .iter().map(|s| s.to_string()).chain([out.to_string_lossy().to_string()]).collect();
        let mut seen = Vec::new();
        run_with_progress(&args, 0, |f| seen.push(f), None).await.unwrap();
        assert_eq!(seen, vec![1.0]);
    }

    #[tokio::test]
    async fn ffmpeg_failure_surfaces_stderr_as_export_error() {
        let args: Vec<String> = ["-i", "/definitely/not/here.mp4", "-f", "null", "-"].iter().map(|s| s.to_string()).collect();
        let err = run_with_progress(&args, 1000, |_| {}, None).await.unwrap_err();
        match err {
            Error::Export(msg) => {
                assert!(msg.contains("ffmpeg exited with"), "{msg}");
                assert!(msg.to_lowercase().contains("no such file"), "{msg}");
            }
            other => panic!("expected Export, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn cancel_kills_a_running_render() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("long.mp4");
        // realtime source so this would take ~60 s if not cancelled
        let args: Vec<String> = ["-y", "-re", "-f", "lavfi", "-i", "testsrc=size=64x64:rate=30:duration=60", "-pix_fmt", "yuv420p"]
            .iter().map(|s| s.to_string()).chain([out.to_string_lossy().to_string()]).collect();
        let (tx, rx) = oneshot::channel();
        let started = std::time::Instant::now();
        let handle = tokio::spawn(async move { run_with_progress(&args, 60_000, |_| {}, Some(rx)).await });
        std::thread::sleep(std::time::Duration::from_millis(400));
        tx.send(()).unwrap();
        let res = handle.await.unwrap();
        assert!(matches!(res, Err(Error::Cancelled)), "{res:?}");
        assert!(started.elapsed().as_secs() < 10);
    }

    #[tokio::test]
    async fn dropping_the_cancel_sender_also_cancels() {
        let args: Vec<String> = ["-re", "-f", "lavfi", "-i", "testsrc=size=64x64:rate=30:duration=60", "-f", "null", "-"]
            .iter().map(|s| s.to_string()).collect();
        let (tx, rx) = oneshot::channel::<()>();
        let handle = tokio::spawn(async move { run_with_progress(&args, 60_000, |_| {}, Some(rx)).await });
        std::thread::sleep(std::time::Duration::from_millis(300));
        drop(tx);
        assert!(matches!(handle.await.unwrap(), Err(Error::Cancelled)));
    }

    #[tokio::test]
    async fn capture_stdout_returns_raw_bytes_and_errors_cleanly() {
        let args: Vec<String> = ["-f", "lavfi", "-i", "sine=frequency=440:duration=0.1", "-ac", "1", "-ar", "8000", "-f", "s16le", "-"]
            .iter().map(|s| s.to_string()).collect();
        let pcm = run_capture_stdout(&args).await.unwrap();
        // 0.1 s × 8000 Hz × 2 bytes = 1600 (allow ffmpeg padding slop)
        assert!((1500..=1800).contains(&pcm.len()), "{}", pcm.len());
        let bad: Vec<String> = ["-i", "/nope.wav", "-f", "s16le", "-"].iter().map(|s| s.to_string()).collect();
        assert!(matches!(run_capture_stdout(&bad).await, Err(Error::Media(_))));
    }
}
