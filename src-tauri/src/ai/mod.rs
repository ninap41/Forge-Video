//! AI mode. Speech becomes text with a local `whisper-cli`; the Claude Code CLI (`claude -p`, the
//! user's own login) reads that text and proposes shorts. Nothing here edits media.

pub mod captions;
pub mod highlights;
pub mod short;
pub mod transcribe;

use crate::error::{Error, Result};
use crate::render::ffmpeg::{find_in, BIN_DIRS};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::{oneshot, watch};

pub const MODEL_FILE: &str = "ggml-base.en.bin";

fn home_dirs() -> Vec<PathBuf> {
    dirs::home_dir().map(|h| vec![h.join(".local/bin"), h.join(".claude/local")]).unwrap_or_default()
}

pub fn whisper_bin() -> Result<PathBuf> {
    find_in("whisper-cli", "FORGE_WHISPER", &[])
        .map_err(|_| Error::Tool("whisper-cli not found. Install it with: brew install whisper.cpp".into()))
}

pub fn claude_bin() -> Result<PathBuf> {
    find_in("claude", "FORGE_CLAUDE", &home_dirs())
        .map_err(|_| Error::Tool("Claude Code not found. Install it, then run `claude` once to log in.".into()))
}

/// Where the speech model is expected: `FORGE_WHISPER_MODEL`, else Application Support.
pub fn model_path() -> PathBuf {
    match std::env::var("FORGE_WHISPER_MODEL") {
        Ok(p) => PathBuf::from(p),
        Err(_) => dirs::data_dir().unwrap_or_else(std::env::temp_dir).join("ForgeVideo/models").join(MODEL_FILE),
    }
}

pub fn whisper_model() -> Result<PathBuf> {
    let p = model_path();
    if p.is_file() { Ok(p) } else { Err(Error::Tool(format!("speech model not found at {}", p.display()))) }
}

/// Who is signed in to Claude Code, from `claude auth status --json` (`email`, else the auth
/// method). `None` when the CLI is missing, not signed in, or answers with something else.
pub fn claude_account(claude: &Path) -> Option<String> {
    let out = std::process::Command::new(claude).args(["auth", "status", "--json"]).output().ok()?;
    let j: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    if j["loggedIn"].as_bool() != Some(true) {
        return None;
    }
    j["email"].as_str().or_else(|| j["authMethod"].as_str()).map(str::to_string)
}

/// The shell line the sign-in Terminal runs: `claude auth login` waits for the browser round trip.
pub fn login_shell_line(claude: &Path) -> String {
    let quoted = format!("'{}'", claude.display().to_string().replace('\'', "'\\''"));
    format!("clear; echo 'Signing in to Claude for ForgeVideo...'; {quoted} auth login && echo 'Done. You can close this window and go back to ForgeVideo.'")
}

/// AppleScript that opens Terminal on `line` and brings it to the front.
pub fn login_applescript(line: &str) -> String {
    let esc = line.replace('\\', "\\\\").replace('"', "\\\"");
    format!("tell application \"Terminal\"\n\tdo script \"{esc}\"\n\tactivate\nend tell")
}

/// Run `line` in a new Terminal.app window (for steps that need a TTY, a browser or a password).
pub fn open_terminal(line: &str) -> Result<()> {
    let script = login_applescript(line);
    let out = std::process::Command::new("osascript").arg("-e").arg(&script).output()
        .map_err(|e| Error::Tool(format!("could not open Terminal: {e}")))?;
    if out.status.success() { Ok(()) } else { Err(Error::Tool(format!("could not open Terminal: {}", String::from_utf8_lossy(&out.stderr).trim()))) }
}

/// Sign-in needs a terminal and a browser, so it runs in Terminal.app; the panel polls `status`
/// until `account` appears.
pub fn claude_login() -> Result<()> {
    open_terminal(&login_shell_line(&claude_bin()?))
}

/// The shell line that runs the bundled `install-ai.sh` (Homebrew, ffmpeg, whisper.cpp, model, Claude Code).
pub fn install_shell_line(script: &Path) -> String {
    let quoted = format!("'{}'", script.display().to_string().replace('\'', "'\\''"));
    format!("clear; /bin/bash {quoted}")
}

/// Opens Terminal on the install script; the panel polls `status` until the tools appear.
pub fn install_tools(script: &Path) -> Result<()> {
    if !script.is_file() {
        return Err(Error::Tool(format!("install script not found at {}", script.display())));
    }
    open_terminal(&install_shell_line(script))
}

pub fn claude_logout() -> Result<()> {
    let claude = claude_bin()?;
    let out = std::process::Command::new(&claude).args(["auth", "logout"]).output()
        .map_err(|e| Error::Tool(format!("could not start claude: {e}")))?;
    if out.status.success() { Ok(()) } else { Err(Error::Tool(format!("sign out failed: {}", String::from_utf8_lossy(&out.stderr).trim()))) }
}

#[derive(Debug, Serialize)]
pub struct AiStatus {
    pub whisper: Option<PathBuf>,
    pub model: Option<PathBuf>,
    /// Where the model should be put when `model` is missing.
    pub model_path: PathBuf,
    pub claude: Option<PathBuf>,
    /// The signed-in Claude account (email), `None` until Matt signs in.
    pub account: Option<String>,
}

pub fn status() -> AiStatus {
    let claude = claude_bin().ok();
    let account = claude.as_deref().and_then(claude_account);
    AiStatus { whisper: whisper_bin().ok(), model: whisper_model().ok(), model_path: model_path(), claude, account }
}

/// One job's cancel signal, shared by every step of the job.
#[derive(Clone)]
pub struct Cancel(watch::Receiver<bool>);

impl Cancel {
    /// Fires when the job registry sends on (or drops) the sender.
    pub fn from_oneshot(rx: oneshot::Receiver<()>) -> Cancel {
        let (tx, wrx) = watch::channel(false);
        tokio::spawn(async move {
            let _ = rx.await;
            let _ = tx.send(true);
        });
        Cancel(wrx)
    }

    pub fn never() -> Cancel {
        Cancel(watch::channel(false).1)
    }

    pub async fn cancelled(&mut self) {
        if self.0.wait_for(|v| *v).await.is_err() {
            std::future::pending::<()>().await
        }
    }

    /// A receiver for `ffmpeg::run_with_progress`, which takes a oneshot per run.
    pub fn oneshot(&self) -> oneshot::Receiver<()> {
        let (mut tx, rx) = oneshot::channel();
        let mut me = self.clone();
        tokio::spawn(async move {
            tokio::select! {
                _ = me.cancelled() => { let _ = tx.send(()); }
                _ = tx.closed() => {}
            }
        });
        rx
    }
}

/// PATH for child tools: `claude` may be a script that needs `node` from Homebrew.
fn child_path() -> String {
    let mut dirs: Vec<String> = BIN_DIRS.iter().map(|d| d.to_string()).collect();
    dirs.extend(home_dirs().iter().map(|d| d.to_string_lossy().to_string()));
    if let Ok(p) = std::env::var("PATH") {
        dirs.push(p);
    }
    dirs.join(":")
}

/// Run a CLI to completion and return its stdout. `on_stderr` sees every stderr line as it arrives.
pub async fn run_tool(
    bin: &Path,
    args: &[String],
    stdin: Option<String>,
    cwd: Option<&Path>,
    mut on_stderr: impl FnMut(&str) + Send,
    mut cancel: Cancel,
) -> Result<String> {
    let name = bin.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let mut cmd = Command::new(bin);
    cmd.args(args)
        .env("PATH", child_path())
        .stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Some(d) = cwd {
        cmd.current_dir(d);
    }
    log::debug!("{name} {}", args.join(" "));
    let mut child = cmd.spawn().map_err(|e| Error::Tool(format!("could not start {name}: {e}")))?;
    if let (Some(text), Some(mut pipe)) = (stdin, child.stdin.take()) {
        tokio::spawn(async move {
            let _ = pipe.write_all(text.as_bytes()).await;
        });
    }
    let mut stdout = child.stdout.take().expect("stdout piped");
    let out_task = tokio::spawn(async move {
        let mut b = Vec::new();
        let _ = stdout.read_to_end(&mut b).await;
        String::from_utf8_lossy(&b).to_string()
    });
    let mut lines = BufReader::new(child.stderr.take().expect("stderr piped")).lines();
    let mut tail: Vec<String> = Vec::new();
    loop {
        tokio::select! {
            line = lines.next_line() => {
                match line {
                    Ok(Some(l)) => {
                        on_stderr(&l);
                        tail.push(l);
                        if tail.len() > 12 { tail.remove(0); }
                    }
                    _ => break,
                }
            }
            _ = cancel.cancelled() => {
                let _ = child.kill().await;
                return Err(Error::Cancelled);
            }
        }
    }
    let status = tokio::select! {
        s = child.wait() => s?,
        _ = cancel.cancelled() => {
            let _ = child.kill().await;
            return Err(Error::Cancelled);
        }
    };
    let out = out_task.await.unwrap_or_default();
    if !status.success() {
        let why = if tail.is_empty() { out.trim().chars().take(400).collect() } else { tail.join("\n") };
        return Err(Error::Tool(format!("{name} exited with {status}: {}", why.trim())));
    }
    Ok(out)
}

#[cfg(test)]
pub mod test_support {
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};

    /// Write an executable shell script standing in for whisper-cli or claude.
    pub fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::script;
    use super::*;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[tokio::test]
    async fn run_tool_returns_stdout_feeds_stdin_and_reports_stderr_lines() {
        let dir = tempfile::tempdir().unwrap();
        let bin = script(dir.path(), "echoer", "echo working >&2\necho \"$1:$(cat)\"\npwd >&2");
        let mut seen = Vec::new();
        let out = run_tool(&bin, &args(&["x"]), Some("hello".into()), Some(dir.path()), |l| seen.push(l.to_string()), Cancel::never()).await.unwrap();
        assert_eq!(out.trim(), "x:hello");
        assert_eq!(seen[0], "working");
        assert!(seen[1].ends_with(dir.path().file_name().unwrap().to_str().unwrap()), "cwd honoured: {seen:?}");
    }

    #[tokio::test]
    async fn run_tool_failure_carries_the_last_stderr_lines() {
        let dir = tempfile::tempdir().unwrap();
        let bin = script(dir.path(), "bad", "echo 'model is broken' >&2\nexit 3");
        let err = run_tool(&bin, &[], None, None, |_| {}, Cancel::never()).await.unwrap_err();
        match err {
            Error::Tool(m) => assert!(m.contains("bad exited with") && m.contains("model is broken"), "{m}"),
            other => panic!("expected Tool, got {other:?}"),
        }
        let missing = run_tool(&dir.path().join("nope"), &[], None, None, |_| {}, Cancel::never()).await.unwrap_err();
        assert!(matches!(missing, Error::Tool(m) if m.contains("could not start nope")));
    }

    #[tokio::test]
    async fn cancel_kills_the_tool_and_is_shared_between_steps() {
        let dir = tempfile::tempdir().unwrap();
        let bin = script(dir.path(), "slow", "sleep 60");
        let (tx, rx) = oneshot::channel();
        let cancel = Cancel::from_oneshot(rx);
        let ffmpeg_rx = cancel.oneshot();
        let started = std::time::Instant::now();
        let c2 = cancel.clone();
        let handle = tokio::spawn(async move { run_tool(&bin, &[], None, None, |_| {}, c2).await });
        std::thread::sleep(std::time::Duration::from_millis(300));
        tx.send(()).unwrap();
        assert!(matches!(handle.await.unwrap(), Err(Error::Cancelled)));
        assert!(started.elapsed().as_secs() < 10);
        assert_eq!(ffmpeg_rx.await, Ok(()), "the ffmpeg step hears the same cancel");
    }

    #[test]
    fn status_reports_overrides_and_missing_tools() {
        let dir = tempfile::tempdir().unwrap();
        let model = dir.path().join("m.bin");
        std::env::set_var("FORGE_WHISPER_MODEL", &model);
        assert!(matches!(whisper_model(), Err(Error::Tool(m)) if m.contains("m.bin")));
        let s = status();
        assert_eq!(s.model, None);
        assert_eq!(s.model_path, model);
        std::fs::write(&model, b"x").unwrap();
        assert_eq!(status().model, Some(model));
        std::env::remove_var("FORGE_WHISPER_MODEL");
        assert!(model_path().ends_with(format!("ForgeVideo/models/{MODEL_FILE}")));
        let j = serde_json::to_value(status()).unwrap();
        assert!(j.get("whisper").is_some() && j.get("claude").is_some() && j["model_path"].is_string());
        assert!(j.get("account").is_some());
    }

    #[test]
    fn account_comes_from_claude_auth_status_json() {
        let dir = tempfile::tempdir().unwrap();
        let signed_in = script(dir.path(), "claude", r#"[ "$1 $2 $3" = "auth status --json" ] || exit 9
echo '{"loggedIn":true,"authMethod":"claude.ai","email":"matt@example.com"}'"#);
        assert_eq!(claude_account(&signed_in), Some("matt@example.com".into()));
        let no_email = script(dir.path(), "claude2", r#"echo '{"loggedIn":true,"authMethod":"console"}'"#);
        assert_eq!(claude_account(&no_email), Some("console".into()));
        let out = script(dir.path(), "claude3", r#"echo '{"loggedIn":false}'"#);
        assert_eq!(claude_account(&out), None);
        let junk = script(dir.path(), "claude4", "echo not json");
        assert_eq!(claude_account(&junk), None);
        assert_eq!(claude_account(&dir.path().join("missing")), None);
    }

    #[test]
    fn login_script_quotes_the_binary_for_sh_and_applescript() {
        let line = login_shell_line(Path::new("/Users/m/it's/claude"));
        assert!(line.contains(r"'/Users/m/it'\''s/claude' auth login"), "{line}");
        let script = login_applescript(&line);
        assert!(script.starts_with("tell application \"Terminal\"\n\tdo script \""), "{script}");
        assert!(script.ends_with("\"\n\tactivate\nend tell"), "{script}");
        // The shell backslash survives doubled, and quotes inside the line are escaped for AppleScript.
        assert!(script.contains(r"/it'\\''s/claude'"), "{script}");
        assert_eq!(login_applescript(r#"say "hi""#), "tell application \"Terminal\"\n\tdo script \"say \\\"hi\\\"\"\n\tactivate\nend tell");
    }

    #[test]
    fn install_line_runs_the_script_with_bash_and_refuses_a_missing_script() {
        let line = install_shell_line(Path::new("/Applications/ForgeVideo.app/Contents/Resources/install-ai.sh"));
        assert_eq!(line, "clear; /bin/bash '/Applications/ForgeVideo.app/Contents/Resources/install-ai.sh'");
        assert!(matches!(install_tools(Path::new("/nope/install-ai.sh")), Err(Error::Tool(m)) if m.contains("install script not found")));
    }

    #[test]
    fn the_repo_install_script_is_valid_bash_and_idempotent_by_design() {
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../scripts/install-ai.sh");
        let body = std::fs::read_to_string(&script).unwrap();
        assert!(body.starts_with("#!/bin/bash") && body.contains("set -euo pipefail"));
        for needle in ["brew install \"$formula\"", "whisper.cpp", "ggml-base.en.bin", "brew install --cask claude-code"] {
            assert!(body.contains(needle), "{needle}");
        }
        let out = std::process::Command::new("/bin/bash").arg("-n").arg(&script).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    }

    #[test]
    fn logout_runs_claude_auth_logout_and_reports_failure() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("logged-out");
        let good = script(dir.path(), "claude", &format!(r#"[ "$1 $2" = "auth logout" ] && touch "{}""#, marker.display()));
        std::env::set_var("FORGE_CLAUDE", &good);
        assert!(claude_logout().is_ok());
        assert!(marker.is_file());
        let bad = script(dir.path(), "claude-bad", "echo 'no session' >&2\nexit 1");
        std::env::set_var("FORGE_CLAUDE", &bad);
        assert!(matches!(claude_logout(), Err(Error::Tool(m)) if m.contains("no session")));
        std::env::remove_var("FORGE_CLAUDE");
    }
}
