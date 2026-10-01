use std::fs;
use std::io;
use std::path::Path;
use std::process::Command;

const SUMMARY_MARKER: &str = "## AppForge Research Summary";
const MAX_CACHED_CHARS: usize = 16_000;

pub fn available() -> bool {
    which("aside")
}

fn which(name: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path)
        .map(|p| p.join(name))
        .any(|p| p.is_file())
}

fn strip_ansi(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b && bytes.get(i + 1) == Some(&b'[') {
            i += 2;
            while i < bytes.len() {
                let b = bytes[i];
                i += 1;
                if (0x40..=0x7e).contains(&b) {
                    break;
                }
            }
            continue;
        }

        let ch = input[i..].chars().next().expect("valid UTF-8 boundary");
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn tail_chars(input: &str, max: usize) -> &str {
    if input.chars().count() <= max {
        return input;
    }
    let start = input
        .char_indices()
        .rev()
        .nth(max.saturating_sub(1))
        .map(|(idx, _)| idx)
        .unwrap_or(0);
    &input[start..]
}

fn compact_output(raw: &str) -> String {
    let clean = strip_ansi(raw);
    let without_diagnostics = clean
        .split_once("\n[aside diagnostics]\n")
        .map(|(body, _)| body)
        .unwrap_or(clean.as_str())
        .trim();

    let summary = if let Some(index) = without_diagnostics.rfind(SUMMARY_MARKER) {
        &without_diagnostics[index..]
    } else if let Some(index) = without_diagnostics.rfind("\n## ") {
        // Legacy Aside runs predate the explicit marker. Raw search previews are
        // JSON-escaped on single lines, while the final response is rendered as
        // real Markdown headings, so the last level-2 heading is a useful boundary.
        &without_diagnostics[index + 1..]
    } else {
        tail_chars(without_diagnostics, MAX_CACHED_CHARS)
    };

    tail_chars(summary.trim(), MAX_CACHED_CHARS)
        .trim()
        .to_string()
}

pub fn cached_research(project: &Path, stage: &str) -> Option<String> {
    let path = project.join("docs/aside").join(format!("{stage}.md"));
    let text = fs::read_to_string(&path).ok()?;
    if text.trim().is_empty() {
        return None;
    }

    let compact = compact_output(&text);
    if compact.is_empty() {
        return None;
    }
    if compact != text {
        let _ = fs::write(&path, &compact);
    }
    Some(compact)
}

pub fn run_research(project: &Path, stage: &str, product_brief: &str) -> io::Result<String> {
    if !available() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "Aside CLI is not installed",
        ));
    }

    let task = match stage {
        "plan" => format!(
            "Research the web for current mobile app/game patterns relevant to this product brief. Focus on market references, store constraints, comparable UX patterns, and implementation risks. Do not purchase, publish, or change account settings. Return concise actionable notes for a coding agent. Product brief: {product_brief}"
        ),
        "design" => format!(
            "Research current mobile game UI/UX references for this brief. Focus on interaction patterns, onboarding, retention loops, accessibility, screen hierarchy, art direction references, and App Store / Play screenshot conventions. Do not copy protected artwork. Return design notes only. Product brief: {product_brief}"
        ),
        "build" => format!(
            "Research current official documentation or reliable implementation references needed to build this mobile app/game. Focus on Expo/React Native where suitable, platform permissions, build constraints, and SDK changes. Return engineering notes and source names; do not change remote accounts. Product brief: {product_brief}"
        ),
        "qa" => format!(
            "Research current mobile QA, privacy, permissions, and store policy checks that may apply to this app/game. Return a concise checklist and risks. Do not submit anything. Product brief: {product_brief}"
        ),
        "store" => format!(
            "Using browser access only for research, inspect current Google Play and Apple App Store listing requirements relevant to this app/game and produce a launch checklist. Do not upload, submit, purchase, or accept legal agreements. Product brief: {product_brief}"
        ),
        "release" => format!(
            "Research the current GitHub Release and distribution checks relevant to this project. If signed-in pages are available, you may inspect them read-only. Do not publish an irreversible store submission. Return a final release verification checklist. Product brief: {product_brief}"
        ),
        _ => format!(
            "Research current web context for this mobile app/game brief and return actionable notes: {product_brief}"
        ),
    };
    let task = format!(
        "{task}\n\nKeep the final answer concise (roughly 1,500 words maximum). Your final response MUST begin with exactly: {SUMMARY_MARKER}. Put only synthesized findings and actionable recommendations after that heading; do not repeat raw search/tool traces."
    );

    let output = Command::new("aside")
        .args(["exec", "--permission", "guard", "--speed", "fast"])
        .arg(task)
        .current_dir(project)
        .output()?;

    let text = compact_output(&String::from_utf8_lossy(&output.stdout));
    let dir = project.join("docs/aside");
    fs::create_dir_all(&dir)?;

    if output.status.success() {
        if text.is_empty() {
            return Err(io::Error::other("Aside returned no research summary"));
        }
        fs::write(dir.join(format!("{stage}.md")), &text)?;
        Ok(text)
    } else {
        let stderr = strip_ansi(&String::from_utf8_lossy(&output.stderr));
        Err(io::Error::other(format!(
            "Aside exited with {}: {}",
            output.status,
            tail_chars(stderr.trim(), 2_000)
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_ansi_control_sequences() {
        assert_eq!(strip_ansi("\x1b[2mThinking\x1b[0m\nDone"), "Thinking\nDone");
    }

    #[test]
    fn marker_discards_tool_trace() {
        let raw = "Thinking: research\nwebsearch(...)\n## AppForge Research Summary\n- useful\n";
        assert_eq!(
            compact_output(raw),
            "## AppForge Research Summary\n- useful"
        );
    }

    #[test]
    fn legacy_cache_keeps_last_markdown_summary() {
        let raw = "Thinking\nPreview {\\\"text\\\":\\\"## raw\\\"}\n## Final Research\n- one\n- two\n[aside diagnostics]\ncreated session";
        assert_eq!(compact_output(raw), "## Final Research\n- one\n- two");
    }
}
