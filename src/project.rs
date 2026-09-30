use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn create(root: &Path, brief: &str) -> io::Result<PathBuf> {
    fs::create_dir_all(root)?;
    let slug = slugify(brief);
    let path = unique_path(root, &slug);
    fs::create_dir_all(path.join("docs/aside"))?;
    fs::create_dir_all(path.join(".appforge"))?;

    fs::write(
        path.join(".appforge/project.conf"),
        format!(
            "brief={}\ncreated_at={}\n",
            brief.replace('\n', " "),
            epoch()
        ),
    )?;

    fs::write(
        path.join("README.md"),
        format!(
            "# {}\n\nGenerated and orchestrated by AppForge.\n\n## Product brief\n\n{}\n\n## Workflow\n\n1. Product plan\n2. UX/game design\n3. Implementation\n4. QA/review\n5. Store preparation\n6. Release preparation\n",
            display_name(&slug),
            brief
        ),
    )?;

    fs::write(path.join("AGENTS.md"), agent_instructions(brief))?;

    fs::write(
        path.join("docs/README.md"),
        "# AppForge working documents\n\nAgents should keep durable planning, architecture, design, QA, release, and store-readiness notes here. Aside Browser research is written under docs/aside/.\n",
    )?;

    let _ = Command::new("git")
        .arg("init")
        .current_dir(&path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    Ok(path)
}

fn agent_instructions(brief: &str) -> String {
    format!(
        r#"# AppForge agent contract

## Product brief

{brief}

## Working rules

- This repository is generated for a shipping mobile app/game, not a throwaway demo.
- Prefer Expo + React Native + TypeScript for casual 2D/mobile games unless the product requirements clearly justify another engine.
- Keep Android and iOS buildability in mind from the first implementation.
- Do not introduce paid APIs, SaaS dependencies, or new credentials unless the product brief requires them.
- Never read or copy authentication tokens from Codex, Claude Code, Cursor, Antigravity, Aside, browser profiles, keychains, or other tools.
- Use the official CLI/tool session already authenticated on the machine.
- Read prior stage documents under docs/ before making changes.
- Keep commits and generated assets deterministic where practical.
- Add tests, lint/typecheck commands, and a reproducible build path.
- Prepare store listing copy, privacy/data notes, icon/screenshot requirements, and release checklist before declaring release-ready.
- Browser-side research or console work may be delegated to Aside Browser; treat its notes as inputs, not unquestioned truth.
- Do not publish an irreversible App Store / Play Store submission without explicit human approval.
- GitHub release automation and build artifacts may be prepared automatically.

## Expected durable documents

- docs/01-product.md
- docs/02-design.md
- docs/03-architecture.md
- docs/04-qa.md
- docs/05-store.md
- docs/06-release.md
"#,
    )
}

pub fn load_brief(project: &Path) -> io::Result<String> {
    let text = fs::read_to_string(project.join(".appforge/project.conf"))?;
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("brief=") {
            return Ok(value.trim().to_string());
        }
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "missing brief in .appforge/project.conf",
    ))
}

pub fn mark_stage(project: &Path, stage: &str, ok: bool, note: &str) -> io::Result<()> {
    let dir = project.join(".appforge");
    fs::create_dir_all(&dir)?;
    fs::write(
        dir.join(format!("stage-{stage}.status")),
        format!(
            "status={}\nupdated_at={}\nnote={}\n",
            if ok { "done" } else { "failed" },
            epoch(),
            note.replace('\n', " ")
        ),
    )
}

fn display_name(slug: &str) -> String {
    slug.split('-')
        .filter(|v| !v.is_empty())
        .map(|v| {
            let mut chars = v.chars();
            match chars.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn unique_path(root: &Path, slug: &str) -> PathBuf {
    let first = root.join(slug);
    if !first.exists() {
        return first;
    }
    root.join(format!("{slug}-{}", epoch()))
}

fn slugify(brief: &str) -> String {
    let mut slug = String::new();
    let mut last_dash = false;
    for ch in brief.chars().take(80) {
        let c = ch.to_ascii_lowercase();
        if c.is_ascii_alphanumeric() {
            slug.push(c);
            last_dash = false;
        } else if (c.is_whitespace() || c == '-' || c == '_') && !slug.is_empty() && !last_dash {
            slug.push('-');
            last_dash = true;
        }
        if slug.len() >= 36 {
            break;
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.len() < 3 {
        format!("game-{}", epoch())
    } else {
        slug
    }
}

fn epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|v| v.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_brief_becomes_slug() {
        assert_eq!(slugify("Neon Snake Runner Game"), "neon-snake-runner-game");
    }

    #[test]
    fn non_ascii_brief_gets_safe_fallback() {
        assert!(slugify("한국형 퍼즐 게임").starts_with("game-"));
    }
}
