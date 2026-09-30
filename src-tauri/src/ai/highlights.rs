//! Asks the Claude Code CLI which parts of the transcript make good shorts. Claude only sees
//! text and has no tools; whatever it answers is validated here before it reaches the project.

use super::captions::timeline_cues;
use super::{run_tool, Cancel};
use crate::error::{Error, Result};
use crate::project::{Highlight, Ms, Project, Range};
use crate::timeline::MIN_CLIP_MS;
use std::path::Path;
use uuid::Uuid;

pub const HIGHLIGHT_MIN_MS: Ms = 10_000;
pub const HIGHLIGHT_MAX_MS: Ms = 180_000;
pub const MAX_HIGHLIGHTS: usize = 10;
const MAX_FADE_MS: Ms = 2_000;
const TITLE_MAX: usize = 80;

const SCHEMA: &str = r#"{"type":"object","required":["highlights"],"properties":{"highlights":{"type":"array","items":{
"type":"object","required":["title","reason","start","end"],"properties":{
"title":{"type":"string"},"reason":{"type":"string"},"start":{"type":"number"},"end":{"type":"number"},
"keep":{"type":"array","items":{"type":"object","required":["start","end"],"properties":{"start":{"type":"number"},"end":{"type":"number"}}}},
"fade_in":{"type":"number"},"fade_out":{"type":"number"},"notes":{"type":"array","items":{"type":"string"}}}}}}}"#;

fn secs(ms: Ms) -> String {
    format!("{}.{}", ms / 1000, ms % 1000 / 100)
}

/// The instructions plus the transcript, one `[start-end] text` line per caption, in seconds.
pub fn build_prompt(p: &Project) -> Result<String> {
    let cues = timeline_cues(p);
    if cues.is_empty() {
        return Err(Error::InvalidEdit("transcribe the timeline first".into()));
    }
    let mut s = format!(
        "You are a short-form video editor. Below is the transcript of a recording that is {total} seconds long. \
Each line is `[start-end] text`, times in seconds.\n\n\
Pick up to {max} sections that would work as standalone vertical shorts: a strong hook in the first seconds, \
one complete idea, a clear payoff, understandable without the rest of the recording. Prefer 20 to 90 seconds; \
never shorter than {min} or longer than {maxlen} seconds. Order them best first.\n\n\
For each section give:\n\
- title: a short working title\n\
- reason: one sentence on why it works\n\
- start, end: seconds, on caption boundaries so no word is cut\n\
- keep: the parts of start..end to keep, in order, as {{start,end}} pairs. Leave out filler, false starts, \
tangents and long pauses. Use a single pair covering the whole section when nothing should be removed.\n\
- fade_in, fade_out: seconds, 0 for a hard cut\n\
- notes: short editing suggestions a person should do by hand (framing, music, an on-screen title)\n\n\
Answer with JSON only, in the form {{\"highlights\":[...]}}.\n\nTranscript:\n",
        total = p.duration_ms() / 1000,
        max = MAX_HIGHLIGHTS,
        min = HIGHLIGHT_MIN_MS / 1000,
        maxlen = HIGHLIGHT_MAX_MS / 1000,
    );
    for c in &cues {
        s.push_str(&format!("[{}-{}] {}\n", secs(c.start), secs(c.end), c.text));
    }
    Ok(s)
}

/// The JSON Claude produced, out of `claude -p --output-format json`'s envelope.
pub fn unwrap_envelope(stdout: &str) -> Result<serde_json::Value> {
    let env: serde_json::Value = serde_json::from_str(stdout.trim())
        .map_err(|_| Error::Tool(format!("Claude returned something unreadable: {}", stdout.trim().chars().take(200).collect::<String>())))?;
    // Newer CLIs print the whole event list; the result is its last entry.
    let env = match env {
        serde_json::Value::Array(mut a) => a.pop().unwrap_or_default(),
        v => v,
    };
    let text = env["result"].as_str().unwrap_or("").to_string();
    if env["is_error"].as_bool().unwrap_or(false) {
        return Err(Error::Tool(format!("Claude could not answer: {}", if text.is_empty() { "unknown error".into() } else { text })));
    }
    if env["structured_output"].is_object() {
        return Ok(env["structured_output"].clone());
    }
    if env["highlights"].is_array() {
        return Ok(env);
    }
    // Plain text answer: take the outermost JSON object, ignoring code fences and prose.
    let (Some(a), Some(b)) = (text.find('{'), text.rfind('}')) else {
        return Err(Error::Tool("Claude answered without JSON".into()));
    };
    serde_json::from_str(&text[a..=b]).map_err(|_| Error::Tool("Claude answered with invalid JSON".into()))
}

fn ms(v: &serde_json::Value) -> Option<Ms> {
    v.as_f64().filter(|s| s.is_finite() && *s >= 0.0).map(|s| (s * 1000.0).round() as Ms)
}

fn short_text(v: &serde_json::Value, max: usize) -> String {
    v.as_str().unwrap_or("").split_whitespace().collect::<Vec<_>>().join(" ").chars().take(max).collect()
}

/// Validate Claude's suggestions against a timeline of `total` ms. Anything unusable is dropped.
pub fn parse_highlights(v: &serde_json::Value, total: Ms) -> Vec<Highlight> {
    let mut out: Vec<Highlight> = Vec::new();
    for h in v["highlights"].as_array().map(Vec::as_slice).unwrap_or(&[]) {
        let (Some(start), Some(end)) = (ms(&h["start"]), ms(&h["end"])) else { continue };
        let end = end.min(total).min(start.saturating_add(HIGHLIGHT_MAX_MS));
        if start + HIGHLIGHT_MIN_MS > end {
            continue;
        }
        let mut keep: Vec<Range> = Vec::new();
        let mut pairs: Vec<(Ms, Ms)> = h["keep"].as_array().map(Vec::as_slice).unwrap_or(&[]).iter()
            .filter_map(|r| Some((ms(&r["start"])?, ms(&r["end"])?)))
            .map(|(s, e)| (s.clamp(start, end), e.clamp(start, end)))
            .collect();
        pairs.sort();
        for (s, e) in pairs {
            match keep.last_mut() {
                Some(last) if s <= last.end => last.end = last.end.max(e),
                _ if s + MIN_CLIP_MS <= e => keep.push(Range { start: s, end: e }),
                _ => {}
            }
        }
        if keep.is_empty() {
            keep.push(Range { start, end });
        }
        let title = short_text(&h["title"], TITLE_MAX);
        out.push(Highlight {
            id: Uuid::new_v4(),
            title: if title.is_empty() { format!("Highlight {}", out.len() + 1) } else { title },
            reason: short_text(&h["reason"], 300),
            start,
            end,
            keep,
            fade_in: ms(&h["fade_in"]).unwrap_or(0).min(MAX_FADE_MS),
            fade_out: ms(&h["fade_out"]).unwrap_or(0).min(MAX_FADE_MS),
            notes: h["notes"].as_array().map(Vec::as_slice).unwrap_or(&[]).iter().map(|n| short_text(n, 200)).filter(|n| !n.is_empty()).take(6).collect(),
        });
        if out.len() == MAX_HIGHLIGHTS {
            break;
        }
    }
    out
}

/// Run Claude Code headless on the prompt. No tools, no MCP servers, nothing saved to disk.
pub async fn find(claude: &Path, prompt: String, total: Ms, cancel: Cancel) -> Result<Vec<Highlight>> {
    let cwd = std::env::temp_dir();
    let args: Vec<String> = ["-p", "--output-format", "json", "--json-schema", SCHEMA, "--tools", "", "--strict-mcp-config", "--no-session-persistence"]
        .iter().map(|s| s.to_string()).collect();
    let out = run_tool(claude, &args, Some(prompt), Some(&cwd), |_| {}, cancel).await?;
    let found = parse_highlights(&unwrap_envelope(&out)?, total);
    if found.is_empty() {
        return Err(Error::Tool("Claude found no section that works as a short".into()));
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::captions::tests::podcast;
    use crate::ai::test_support::script;
    use serde_json::json;

    #[test]
    fn prompt_lists_the_timeline_captions_in_seconds() {
        let mut p = podcast();
        let s = build_prompt(&p).unwrap();
        assert!(s.contains("90 seconds long"));
        assert!(s.contains("[0.0-9.0] /a.mp4 0\n"));
        assert!(s.contains("[60.0-69.0] /b.mp4 0\n"), "timeline time, not source time");
        assert!(s.trim_end().ends_with("/b.mp4 2"));
        p.transcripts.clear();
        assert!(matches!(build_prompt(&p), Err(Error::InvalidEdit(_))));
    }

    #[test]
    fn schema_is_valid_json() {
        let v: serde_json::Value = serde_json::from_str(SCHEMA).unwrap();
        assert_eq!(v["required"][0], "highlights");
    }

    #[test]
    fn envelope_forms_are_all_understood() {
        let h = json!({"highlights": [{"title": "t", "reason": "r", "start": 1, "end": 40}]});
        let structured = json!({"type": "result", "is_error": false, "result": "", "structured_output": h}).to_string();
        assert_eq!(unwrap_envelope(&structured).unwrap(), h);
        let events = json!([{"type": "system"}, {"type": "result", "is_error": false, "structured_output": h}]).to_string();
        assert_eq!(unwrap_envelope(&events).unwrap(), h);
        let fenced = json!({"type": "result", "is_error": false, "result": format!("Here you go:\n```json\n{h}\n```")}).to_string();
        assert_eq!(unwrap_envelope(&fenced).unwrap(), h);
        assert_eq!(unwrap_envelope(&h.to_string()).unwrap(), h, "bare JSON");

        let failed = json!({"type": "result", "is_error": true, "result": "Invalid API key · Please run /login"}).to_string();
        assert!(matches!(unwrap_envelope(&failed), Err(Error::Tool(m)) if m.contains("Please run /login")));
        assert!(matches!(unwrap_envelope(&json!({"result": "I cannot help"}).to_string()), Err(Error::Tool(m)) if m.contains("without JSON")));
        assert!(matches!(unwrap_envelope(&json!({"result": "{broken}"}).to_string()), Err(Error::Tool(m)) if m.contains("invalid JSON")));
        assert!(matches!(unwrap_envelope("zsh: command not found"), Err(Error::Tool(m)) if m.contains("unreadable")));
    }

    #[test]
    fn suggestions_are_clamped_merged_and_filtered() {
        let v = json!({"highlights": [
            {"title": "  The   hook \n", "reason": "Strong open", "start": 10.0, "end": 55.5,
             "keep": [{"start": 30, "end": 55.5}, {"start": 10, "end": 20.04}, {"start": 19, "end": 25}, {"start": 5, "end": 9}, {"start": 40, "end": 40.05}],
             "fade_in": 0.5, "fade_out": 30, "notes": ["Add a title", "", 7]},
            {"title": "too short", "reason": "", "start": 10, "end": 15},
            {"title": "", "reason": "", "start": 80, "end": 500},
            {"title": "past the end", "reason": "", "start": 95, "end": 120},
            {"title": "backwards", "reason": "", "start": 50, "end": 20},
            {"title": "garbage", "reason": "", "start": "soon", "end": null},
            {"title": "negative", "reason": "", "start": -5, "end": 30},
            {"title": "long", "reason": "", "start": 0, "end": 90}
        ]});
        let hs = parse_highlights(&v, 90_000);
        assert_eq!(hs.iter().map(|h| h.title.as_str()).collect::<Vec<_>>(), vec!["The hook", "Highlight 2", "long"]);
        let h = &hs[0];
        assert_eq!((h.start, h.end), (10_000, 55_500));
        assert_eq!(h.keep, vec![Range { start: 10_000, end: 25_000 }, Range { start: 30_000, end: 55_500 }], "sorted, merged, slivers dropped");
        assert_eq!((h.fade_in, h.fade_out), (500, 2_000));
        assert_eq!(h.notes, vec!["Add a title"]);
        assert_eq!((hs[1].start, hs[1].end), (80_000, 90_000), "end clamped to the timeline");
        assert_eq!(hs[1].keep, vec![Range { start: 80_000, end: 90_000 }], "no keep list = the whole section");

        let long = parse_highlights(&json!({"highlights": [{"title": "x", "start": 0, "end": 900}]}), 3_600_000);
        assert_eq!(long[0].end, HIGHLIGHT_MAX_MS);
        let many: Vec<_> = (0..20).map(|i| json!({"title": format!("h{i}"), "start": i * 20, "end": i * 20 + 15})).collect();
        assert_eq!(parse_highlights(&json!({"highlights": many}), 3_600_000).len(), MAX_HIGHLIGHTS);
        assert!(parse_highlights(&json!({"highlights": "none"}), 90_000).is_empty());
        assert!(parse_highlights(&json!({}), 90_000).is_empty());
    }

    #[tokio::test]
    async fn find_sends_the_prompt_on_stdin_with_tools_disabled() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("seen.txt");
        let claude = script(dir.path(), "claude", &format!(
            "{{ echo \"$@\"; cat; }} > '{}'\n\
             echo '{{\"type\":\"result\",\"is_error\":false,\"result\":\"\",\"structured_output\":{{\"highlights\":[{{\"title\":\"Hook\",\"reason\":\"r\",\"start\":5,\"end\":45}}]}}}}'",
            log.display()));
        let hs = find(&claude, "the transcript".into(), 90_000, Cancel::never()).await.unwrap();
        assert_eq!((hs.len(), hs[0].title.as_str(), hs[0].start, hs[0].end), (1, "Hook", 5_000, 45_000));
        let seen = std::fs::read_to_string(&log).unwrap();
        assert!(seen.starts_with("-p --output-format json --json-schema {"), "{seen}");
        assert!(seen.contains("--tools  --strict-mcp-config --no-session-persistence"), "empty tool list: {seen}");
        assert!(seen.trim_end().ends_with("the transcript"));
    }

    #[tokio::test]
    async fn find_reports_failures_and_empty_answers() {
        let dir = tempfile::tempdir().unwrap();
        let none = script(dir.path(), "claude", "cat > /dev/null; echo '{\"is_error\":false,\"structured_output\":{\"highlights\":[]}}'");
        assert!(matches!(find(&none, "t".into(), 90_000, Cancel::never()).await, Err(Error::Tool(m)) if m.contains("no section")));
        let down = script(dir.path(), "claude2", "cat > /dev/null; echo 'network unreachable' >&2; exit 1");
        assert!(matches!(find(&down, "t".into(), 90_000, Cancel::never()).await, Err(Error::Tool(m)) if m.contains("network unreachable")));
        let slow = script(dir.path(), "claude3", "sleep 60");
        let (tx, rx) = tokio::sync::oneshot::channel();
        let cancel = Cancel::from_oneshot(rx);
        let handle = tokio::spawn(async move { find(&slow, "t".into(), 90_000, cancel).await });
        std::thread::sleep(std::time::Duration::from_millis(300));
        tx.send(()).unwrap();
        assert!(matches!(handle.await.unwrap(), Err(Error::Cancelled)));
    }
}
