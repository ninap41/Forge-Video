//! Font families the text track can use. The webview draws titles with CSS family names, so we
//! only need names: `fc-list : family` (fontconfig comes with the Homebrew ffmpeg) lists every
//! installed font; without it a short fixed list of macOS fonts is offered.

use crate::render::ffmpeg::find_in;
use std::process::Command;

/// Fonts bundled with the app (`src/style.css` @font-face), always offered first.
pub const BUNDLED: [&str; 2] = ["Quicksand", "Orbit"];
const FALLBACK: [&str; 8] = ["Arial", "Avenir Next", "Futura", "Georgia", "Helvetica Neue", "Impact", "Menlo", "Times New Roman"];

/// One family per `fc-list` line. A line may carry comma-separated aliases (localised names
/// first, e.g. `ديوان كوفي,Diwan Kufi`); the first ASCII alias wins.
pub fn parse_fc_list(out: &str) -> Vec<String> {
    let mut names: Vec<String> = out
        .lines()
        .filter_map(|line| {
            let aliases: Vec<&str> = line.split(',').map(str::trim).filter(|a| !a.is_empty()).collect();
            aliases.iter().find(|a| a.is_ascii()).or(aliases.first()).map(|a| a.to_string())
        })
        .filter(|n| !n.starts_with('.'))
        .collect();
    names.sort_by_key(|n| n.to_lowercase());
    names.dedup_by_key(|n| n.to_lowercase());
    names
}

fn with_bundled(mut names: Vec<String>) -> Vec<String> {
    names.retain(|n| !BUNDLED.iter().any(|b| b.eq_ignore_ascii_case(n)));
    BUNDLED.iter().map(|s| s.to_string()).chain(names).collect()
}

/// Every installed family, bundled fonts first, or the fallback list when `fc-list` is missing.
pub fn system_fonts() -> Vec<String> {
    let listed = find_in("fc-list", "FORGE_FCLIST", &[])
        .ok()
        .and_then(|bin| Command::new(bin).args([":", "family"]).output().ok())
        .filter(|o| o.status.success())
        .map(|o| parse_fc_list(&String::from_utf8_lossy(&o.stdout)))
        .filter(|v| !v.is_empty());
    with_bundled(listed.unwrap_or_else(|| FALLBACK.iter().map(|s| s.to_string()).collect()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fc_list_lines_become_sorted_unique_ascii_names() {
        let out = "ديوان كوفي,Diwan Kufi\nHelvetica Neue\n\nArial\narial\n.SF NS Mono\nヒラギノ角ゴシック\nHelvetica Neue\nQuicksand\n";
        assert_eq!(parse_fc_list(out), ["Arial", "Diwan Kufi", "Helvetica Neue", "Quicksand", "ヒラギノ角ゴシック"]);
        assert_eq!(with_bundled(parse_fc_list(out)), ["Quicksand", "Orbit", "Arial", "Diwan Kufi", "Helvetica Neue", "ヒラギノ角ゴシック"]);
        let fb = with_bundled(FALLBACK.iter().map(|s| s.to_string()).collect());
        assert_eq!(fb[..2], ["Quicksand", "Orbit"]);
        assert_eq!(fb.len(), 10);
    }
}
