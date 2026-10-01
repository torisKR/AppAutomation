use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Component, Path, PathBuf};

use crate::config::Config;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn create(root: &Path, brief: &str) -> io::Result<PathBuf> {
    fs::create_dir_all(root)?;
    let slug = slugify(brief);
    let mut suffix = 0;
    let path = loop {
        let candidate = if suffix == 0 {
            root.join(&slug)
        } else {
            root.join(format!("{slug}-{suffix}"))
        };
        match fs::create_dir(&candidate) {
            Ok(()) => break candidate,
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => suffix += 1,
            Err(err) => return Err(err),
        }
    };
    fs::create_dir_all(path.join("docs/aside"))?;
    fs::create_dir_all(path.join("docs/policies"))?;
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
            "# {}\n\nGenerated and orchestrated by AppForge.\n\n## Product brief\n\n{}\n\n## Workflow\n\n1. Product plan\n2. UX/game design\n3. Implementation\n4. Functional & performance repair\n5. QA/review\n6. Store & policy preparation\n7. Notion/store draft upload\n8. Release preparation\n",
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
- Treat docs/04-quality.md as a repair gate: find functional and measurable performance weaknesses, fix verified issues, then record before/after evidence.
- Generate policy documents from verified code/data behavior under docs/policies/; never invent privacy claims.
- Prepare store listing copy, privacy/data notes, icon/screenshot requirements, and release checklist before declaring release-ready.
- Browser-side research or console work may be delegated to Aside Browser; treat its notes as inputs, not unquestioned truth.
- CUA Driver may be used through Codex or Claude for runtime checks, Notion policy publishing, and draft store uploads.
- Never publish a configured Notion parent page; create a dedicated policy child/page and expose only the intended policy content.
- Draft store metadata/build uploads are allowed when configured, but do not submit for review, release to production, accept agreements, or create irreversible store identifiers.
- GitHub release automation and build artifacts may be prepared automatically.

## Expected durable documents

- docs/01-product.md
- docs/02-design.md
- docs/03-architecture.md
- docs/04-quality.md
- docs/05-qa.md
- docs/06-store.md
- docs/07-publish.md
- docs/08-release.md
- docs/policies/privacy-policy.md
- docs/policies/terms.md
- docs/policies/support-and-data-deletion.md
"#,
    )
}

#[derive(Debug)]
pub struct StageLock {
    path: PathBuf,
}

impl Drop for StageLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn process_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }

    #[cfg(windows)]
    {
        Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .stdin(Stdio::null())
            .output()
            .map(|output| {
                output.status.success()
                    && String::from_utf8_lossy(&output.stdout).contains(&pid.to_string())
            })
            .unwrap_or(false)
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
        false
    }
}

pub fn acquire_stage_lock(project: &Path, stage: &str) -> io::Result<StageLock> {
    let dir = project.join(".appforge");
    fs::create_dir_all(&dir)?;
    let path = dir.join("stage.lock");

    for _ in 0..2 {
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => {
                writeln!(file, "pid={}", std::process::id())?;
                writeln!(file, "stage={stage}")?;
                writeln!(file, "started_at={}", epoch())?;
                return Ok(StageLock { path });
            }
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
                let current = fs::read_to_string(&path).unwrap_or_default();
                let pid = current
                    .lines()
                    .find_map(|line| line.strip_prefix("pid="))
                    .and_then(|value| value.parse::<u32>().ok());
                if pid.is_some_and(process_alive) {
                    return Err(io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        format!(
                            "another AppForge stage is already running for this project ({})",
                            current.lines().collect::<Vec<_>>().join(", ")
                        ),
                    ));
                }
                fs::remove_file(&path)?;
            }
            Err(err) => return Err(err),
        }
    }

    Err(io::Error::other("could not acquire AppForge stage lock"))
}

pub fn discover(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut projects = Vec::new();
    if !root.exists() {
        return Ok(projects);
    }
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() && path.join(".appforge/project.conf").is_file() {
            projects.push(path);
        }
    }
    projects.sort();
    Ok(projects)
}

pub fn load_brief(project: &Path) -> io::Result<String> {
    let text = fs::read_to_string(project.join(".appforge/project.conf"))?;
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("brief=") {
            if !value.trim().is_empty() {
                return Ok(value.trim().to_string());
            }
        }
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "missing brief in .appforge/project.conf",
    ))
}

pub fn update_brief(project: &Path, brief: &str) -> io::Result<()> {
    let brief = brief.trim();
    if brief.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "product brief cannot be empty",
        ));
    }

    let conf_path = project.join(".appforge/project.conf");
    let current = fs::read_to_string(&conf_path)?;
    let mut found = false;
    let mut lines = Vec::new();
    for line in current.lines() {
        if line.starts_with("brief=") {
            lines.push(format!("brief={}", brief.replace(['\n', '\r'], " ")));
            found = true;
        } else {
            lines.push(line.to_string());
        }
    }
    if !found {
        lines.insert(0, format!("brief={}", brief.replace(['\n', '\r'], " ")));
    }
    fs::write(conf_path, format!("{}\n", lines.join("\n")))?;

    fs::write(project.join("AGENTS.md"), agent_instructions(brief))?;

    let readme_path = project.join("README.md");
    if let Ok(readme) = fs::read_to_string(&readme_path) {
        const START: &str = "## Product brief\n\n";
        const END: &str = "\n\n## Workflow";
        if let Some(start) = readme.find(START) {
            let content_start = start + START.len();
            if let Some(relative_end) = readme[content_start..].find(END) {
                let content_end = content_start + relative_end;
                let mut updated = String::with_capacity(readme.len() + brief.len());
                updated.push_str(&readme[..content_start]);
                updated.push_str(brief);
                updated.push_str(&readme[content_end..]);
                fs::write(readme_path, updated)?;
            }
        }
    }

    // A changed brief invalidates browser research and every downstream stage result.
    let aside_dir = project.join("docs/aside");
    if let Ok(entries) = fs::read_dir(&aside_dir) {
        for entry in entries.flatten() {
            if entry
                .file_type()
                .map(|kind| kind.is_file())
                .unwrap_or(false)
            {
                let _ = fs::remove_file(entry.path());
            }
        }
    }

    let appforge_dir = project.join(".appforge");
    if let Ok(entries) = fs::read_dir(&appforge_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with("stage-")
                || matches!(
                    name.as_ref(),
                    "quality-decision"
                        | "qa-decision"
                        | "release-decision"
                        | "publish-approved.conf"
                        | "store-upload-request.conf"
                        | "policy-links.conf"
                )
            {
                let _ = fs::remove_file(entry.path());
            }
        }
    }

    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct StoreUploadRequest {
    app_identifier: String,
    allowed_actions: String,
    artifacts: Vec<(String, String)>,
}

fn parse_store_upload_request(text: &str) -> io::Result<StoreUploadRequest> {
    let mut app_identifier = None;
    let mut allowed_actions = None;
    let mut artifacts = Vec::new();

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line == "approved=false" {
            continue;
        }
        if let Some(value) = line.strip_prefix("app_identifier=") {
            let value = value.trim();
            if value.is_empty() || value.chars().any(char::is_control) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid app_identifier in store upload request",
                ));
            }
            app_identifier = Some(value.to_string());
            continue;
        }
        if let Some(value) = line.strip_prefix("allowed_actions=") {
            let requested = value
                .split(',')
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .collect::<Vec<_>>();
            let allowed = ["metadata", "policy_urls", "screenshots", "build_upload"];
            if requested.is_empty() || requested.iter().any(|v| !allowed.contains(v)) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "store upload request contains an unsupported action",
                ));
            }
            allowed_actions = Some(requested.join(","));
            continue;
        }
        if let Some(value) = line.strip_prefix("artifact=") {
            let Some((path, sha)) = value.split_once("|sha256=") else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "artifact entry must use artifact=<path>|sha256=<hex>",
                ));
            };
            let path = path.trim();
            let sha = sha.trim();
            let artifact_path = Path::new(path);
            if path.is_empty()
                || artifact_path.is_absolute()
                || artifact_path.components().any(|component| {
                    matches!(
                        component,
                        Component::ParentDir | Component::RootDir | Component::Prefix(_)
                    )
                })
                || sha.len() != 64
                || !sha.bytes().all(|b| b.is_ascii_hexdigit())
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid artifact path or SHA-256 in store upload request",
                ));
            }
            artifacts.push((path.to_string(), sha.to_ascii_lowercase()));
            continue;
        }

        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unsupported store upload request field: {line}"),
        ));
    }

    Ok(StoreUploadRequest {
        app_identifier: app_identifier.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "store upload request is missing app_identifier",
            )
        })?,
        allowed_actions: allowed_actions.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "store upload request is missing allowed_actions",
            )
        })?,
        artifacts,
    })
}

fn load_store_upload_request(project: &Path) -> io::Result<StoreUploadRequest> {
    let text = fs::read_to_string(project.join(".appforge/store-upload-request.conf"))?;
    let request = parse_store_upload_request(&text)?;
    if (request
        .allowed_actions
        .split(',')
        .any(|v| v == "build_upload")
        || request
            .allowed_actions
            .split(',')
            .any(|v| v == "screenshots"))
        && request.artifacts.is_empty()
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "screenshot/build upload approval requires at least one hashed artifact",
        ));
    }
    verify_store_artifacts(project, &request)?;
    Ok(request)
}

fn verify_store_artifacts(project: &Path, request: &StoreUploadRequest) -> io::Result<()> {
    let root = project.canonicalize()?;
    for (relative, expected_sha) in &request.artifacts {
        let artifact = project.join(relative).canonicalize().map_err(|err| {
            io::Error::new(
                err.kind(),
                format!("approved artifact {relative} cannot be resolved: {err}"),
            )
        })?;
        if !artifact.starts_with(&root) || !artifact.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("approved artifact escapes project or is not a file: {relative}"),
            ));
        }
        let actual_sha = file_sha256(&artifact)?;
        if actual_sha != *expected_sha {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "artifact SHA-256 changed for {relative}: request={expected_sha} actual={actual_sha}"
                ),
            ));
        }
    }
    Ok(())
}

fn file_sha256(path: &Path) -> io::Result<String> {
    #[cfg(target_os = "windows")]
    let output = Command::new("certutil")
        .arg("-hashfile")
        .arg(path)
        .arg("SHA256")
        .output()?;

    #[cfg(target_os = "macos")]
    let output = Command::new("shasum")
        .args(["-a", "256"])
        .arg(path)
        .output()?;

    #[cfg(all(unix, not(target_os = "macos")))]
    let output = Command::new("sha256sum").arg(path).output()?;

    #[cfg(not(any(unix, target_os = "windows")))]
    return Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "SHA-256 verification is unsupported on this OS",
    ));

    if !output.status.success() {
        return Err(io::Error::other(format!(
            "SHA-256 tool failed for {}: {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }

    let text = String::from_utf8_lossy(&output.stdout);
    text.split_whitespace()
        .map(|token| token.trim())
        .find(|token| token.len() == 64 && token.bytes().all(|b| b.is_ascii_hexdigit()))
        .map(str::to_ascii_lowercase)
        .ok_or_else(|| io::Error::other("SHA-256 tool returned no 64-hex digest"))
}

fn single_line(value: &str) -> io::Result<&str> {
    if value.chars().any(char::is_control) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "approval value contains control characters",
        ));
    }
    Ok(value)
}

pub fn approve_publish(project: &Path, cfg: &Config) -> io::Result<()> {
    if !io::stdin().is_terminal() {
        return Err(io::Error::other(
            "publish approval requires an interactive terminal",
        ));
    }
    if !cfg.notion_enabled && !cfg.store_draft_upload {
        return Err(io::Error::other(
            "Notion publishing and store draft upload are both disabled",
        ));
    }

    let store_request = if cfg.store_draft_upload {
        Some(load_store_upload_request(project)?)
    } else {
        None
    };

    println!("\nAppForge external publish approval");
    println!("==================================");
    println!("Project: {}", project.display());
    if cfg.notion_enabled {
        println!(
            "Notion: create/update app policy content under {} target {}",
            cfg.notion_target_kind, cfg.notion_target_url
        );
        println!(
            "Notion public publish: {}",
            if cfg.notion_publish_public {
                "yes"
            } else {
                "no"
            }
        );
    }
    if let Some(request) = &store_request {
        println!("Store app identifier: {}", request.app_identifier);
        println!("Allowed draft actions: {}", request.allowed_actions);
        if request.artifacts.is_empty() {
            println!("Artifacts: none requested");
        } else {
            println!("Artifacts:");
            for (path, sha) in &request.artifacts {
                println!("  - {path}  sha256={sha}");
            }
        }
    }
    println!(
        "Final review submission, production rollout, agreements, pricing, and account changes remain forbidden."
    );
    print!("Approve exactly these external actions for one publish attempt? [y/N]: ");
    io::stdout().flush()?;
    let mut answer = String::new();
    if io::stdin().read_line(&mut answer)? == 0
        || !matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
    {
        return Err(io::Error::other("publish approval was not granted"));
    }

    let mut approval = format!(
        "approved=true\napproved_at={}\nnotion_enabled={}\nnotion_target_kind={}\nnotion_target_url={}\nnotion_publish_public={}\nstore_draft_upload={}\n",
        epoch(),
        cfg.notion_enabled,
        single_line(&cfg.notion_target_kind)?,
        single_line(&cfg.notion_target_url)?,
        cfg.notion_publish_public,
        cfg.store_draft_upload,
    );
    if let Some(request) = &store_request {
        approval.push_str(&format!(
            "app_identifier={}\nallowed_actions={}\n",
            single_line(&request.app_identifier)?,
            request.allowed_actions
        ));
        for (path, sha) in &request.artifacts {
            approval.push_str(&format!("artifact={path}|sha256={sha}\n"));
        }
    }
    fs::write(project.join(".appforge/publish-approved.conf"), approval)?;
    println!("Approved for one publish attempt.");
    Ok(())
}

pub fn validate_publish_approval(project: &Path, cfg: &Config) -> io::Result<()> {
    let text = fs::read_to_string(project.join(".appforge/publish-approved.conf"))?;
    let required = [
        ("approved", "true".to_string()),
        ("notion_enabled", cfg.notion_enabled.to_string()),
        ("notion_target_kind", cfg.notion_target_kind.clone()),
        ("notion_target_url", cfg.notion_target_url.clone()),
        (
            "notion_publish_public",
            cfg.notion_publish_public.to_string(),
        ),
        ("store_draft_upload", cfg.store_draft_upload.to_string()),
    ];
    for (key, expected) in required {
        let found = text
            .lines()
            .find_map(|line| line.strip_prefix(&format!("{key}=")));
        if found != Some(expected.as_str()) {
            return Err(io::Error::other(format!(
                "publish approval is missing or stale for {key}"
            )));
        }
    }

    if cfg.store_draft_upload {
        let request = load_store_upload_request(project)?;
        for expected in [
            format!("app_identifier={}", request.app_identifier),
            format!("allowed_actions={}", request.allowed_actions),
        ] {
            if !text.lines().any(|line| line == expected) {
                return Err(io::Error::other(
                    "publish approval does not match the current store request",
                ));
            }
        }
        for (path, sha) in request.artifacts {
            let expected = format!("artifact={path}|sha256={sha}");
            if !text.lines().any(|line| line == expected) {
                return Err(io::Error::other(
                    "publish approval does not match the current artifact request",
                ));
            }
        }
    }
    Ok(())
}

pub fn consume_publish_approval(project: &Path) -> io::Result<()> {
    match fs::remove_file(project.join(".appforge/publish-approved.conf")) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

pub fn stage_done(project: &Path, stage: &str) -> bool {
    fs::read_to_string(
        project
            .join(".appforge")
            .join(format!("stage-{stage}.status")),
    )
    .map(|text| text.lines().any(|line| line.trim() == "status=done"))
    .unwrap_or(false)
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

    #[test]
    fn update_brief_rewrites_agent_and_project_sources() {
        let root = std::env::temp_dir().join(format!("appforge-brief-test-{}", epoch()));
        let project = create(&root, "old brief").unwrap();
        update_brief(
            &project,
            "미국 시장용 2D 퍼즐 게임. 한 손 조작과 Daily Puzzle을 포함한다.",
        )
        .unwrap();

        let loaded = load_brief(&project).unwrap();
        assert!(loaded.contains("Daily Puzzle"));
        let agents = fs::read_to_string(project.join("AGENTS.md")).unwrap();
        assert!(agents.contains("Daily Puzzle"));
        let readme = fs::read_to_string(project.join("README.md")).unwrap();
        assert!(readme.contains("Daily Puzzle"));
        assert!(!readme.contains("## Product brief\n\nold brief"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn stage_lock_blocks_duplicate_workers_and_recovers_after_drop() {
        let root = std::env::temp_dir().join(format!("appforge-lock-test-{}", epoch()));
        let project = create(&root, "lock test").unwrap();

        let first = acquire_stage_lock(&project, "quality").unwrap();
        let second = acquire_stage_lock(&project, "qa").unwrap_err();
        assert_eq!(second.kind(), io::ErrorKind::AlreadyExists);

        drop(first);
        let third = acquire_stage_lock(&project, "qa").unwrap();
        drop(third);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn store_upload_request_rejects_path_traversal_and_unknown_actions() {
        let traversal = parse_store_upload_request(
            "app_identifier=android:com.example.game\nallowed_actions=build_upload\nartifact=../game.aab|sha256=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\napproved=false\n",
        );
        assert!(traversal.is_err());

        let unknown = parse_store_upload_request(
            "app_identifier=android:com.example.game\nallowed_actions=metadata,production_release\napproved=false\n",
        );
        assert!(unknown.is_err());
    }

    #[test]
    fn store_upload_request_parses_bounded_draft_actions() {
        let request = parse_store_upload_request(
            "app_identifier=android:com.example.game\nallowed_actions=metadata,policy_urls\napproved=false\n",
        )
        .unwrap();
        assert_eq!(request.app_identifier, "android:com.example.game");
        assert_eq!(request.allowed_actions, "metadata,policy_urls");
        assert!(request.artifacts.is_empty());
    }
}
