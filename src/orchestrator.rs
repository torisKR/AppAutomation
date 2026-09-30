use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::thread;

use crate::aside;
use crate::computer;
use crate::config::Config;
use crate::project;
use crate::provider;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Plan,
    Design,
    Build,
    Quality,
    Qa,
    Store,
    Publish,
    Release,
}

impl Stage {
    pub fn all() -> &'static [Stage] {
        &[
            Stage::Plan,
            Stage::Design,
            Stage::Build,
            Stage::Quality,
            Stage::Qa,
            Stage::Store,
            Stage::Publish,
            Stage::Release,
        ]
    }

    pub fn repair() -> &'static [Stage] {
        &[Stage::Quality, Stage::Qa]
    }

    pub fn id(self) -> &'static str {
        match self {
            Stage::Plan => "plan",
            Stage::Design => "design",
            Stage::Build => "build",
            Stage::Quality => "quality",
            Stage::Qa => "qa",
            Stage::Store => "store",
            Stage::Publish => "publish",
            Stage::Release => "release",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Stage::Plan => "01 Product plan",
            Stage::Design => "02 UX / game design",
            Stage::Build => "03 Development",
            Stage::Quality => "04 Functional & performance repair",
            Stage::Qa => "05 QA & review",
            Stage::Store => "06 Store & policy readiness",
            Stage::Publish => "07 Notion & store draft upload",
            Stage::Release => "08 Release",
        }
    }
}

#[derive(Clone, Debug)]
pub enum WorkerEvent {
    Log(String),
    Completed {
        stage: Stage,
        provider: String,
        result: Result<(), String>,
    },
}

pub fn provider_for(cfg: &Config, stage: Stage) -> String {
    let candidate = match stage {
        Stage::Plan | Stage::Quality | Stage::Qa | Stage::Publish | Stage::Release => {
            cfg.primary.clone()
        }
        Stage::Design | Stage::Store => cfg
            .secondary
            .first()
            .cloned()
            .unwrap_or_else(|| cfg.primary.clone()),
        Stage::Build => cfg
            .secondary
            .get(1)
            .or_else(|| cfg.secondary.first())
            .cloned()
            .unwrap_or_else(|| cfg.primary.clone()),
    };

    if cfg.strict_subscription_auth && candidate == "opencode" {
        cfg.enabled
            .iter()
            .find(|id| id.as_str() != "opencode")
            .cloned()
            .unwrap_or(candidate)
    } else {
        candidate
    }
}

fn uses_aside(stage: Stage) -> bool {
    matches!(
        stage,
        Stage::Plan | Stage::Design | Stage::Store | Stage::Release
    )
}

pub fn spawn_stage(
    cfg: Config,
    project_path: PathBuf,
    product_brief: String,
    stage: Stage,
    tx: Sender<WorkerEvent>,
) {
    thread::spawn(move || {
        let quality_computer = stage == Stage::Quality
            && computer::controller(&cfg).is_some()
            && computer::status().ready();
        let provider_id = if stage == Stage::Publish || quality_computer {
            computer::controller(&cfg).unwrap_or_else(|| provider_for(&cfg, stage))
        } else {
            provider_for(&cfg, stage)
        };
        let _ = tx.send(WorkerEvent::Log(format!(
            "{} → {}",
            stage.title(),
            provider::label(&provider_id)
        )));

        if cfg.aside_enabled && uses_aside(stage) {
            let _ = tx.send(WorkerEvent::Log(format!(
                "Aside Browser ▶ {} research lane",
                stage.id()
            )));
            match aside::run_research(&project_path, stage.id(), &product_brief) {
                Ok(text) => {
                    let summary = text
                        .lines()
                        .find(|line| !line.trim().is_empty())
                        .unwrap_or("notes saved");
                    let _ = tx.send(WorkerEvent::Log(format!(
                        "Aside ✓ {}",
                        truncate(summary, 120)
                    )));
                }
                Err(err) => {
                    let _ = tx.send(WorkerEvent::Log(format!("Aside ! non-blocking: {err}")));
                }
            }
        }

        let (log_tx, log_rx) = mpsc::channel::<String>();
        let forward_tx = tx.clone();
        let forwarder = thread::spawn(move || {
            while let Ok(line) = log_rx.recv() {
                let _ = forward_tx.send(WorkerEvent::Log(line));
            }
        });

        let gate = match stage {
            Stage::Quality => Some("quality"),
            Stage::Qa => Some("qa"),
            _ => None,
        };
        let preparation = (|| -> io::Result<()> {
            if stage == Stage::Quality {
                match std::fs::remove_file(project_path.join(".appforge/qa-decision")) {
                    Ok(()) => {}
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e),
                }
            }
            if let Some(gate) = gate {
                let file = project_path.join(format!(".appforge/{gate}-decision"));
                match std::fs::remove_file(file) {
                    Ok(()) => {}
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e),
                }
            }
            if matches!(stage, Stage::Publish | Stage::Release) {
                verify_gates(&project_path)?;
            }
            if stage == Stage::Publish
                && cfg.notion_enabled
                && (!matches!(cfg.notion_target_kind.as_str(), "page" | "database")
                    || !valid_notion_url(&cfg.notion_target_url))
            {
                return Err(io::Error::other(
                    "invalid Notion target type or HTTPS Notion URL",
                ));
            }
            if stage == Stage::Publish && (cfg.notion_enabled || cfg.store_draft_upload) {
                project::validate_publish_approval(&project_path, &cfg)?;
            }
            Ok(())
        })();
        let result = if let Err(err) = preparation {
            Err(err.to_string())
        } else {
            match stage {
                Stage::Publish if !cfg.notion_enabled && !cfg.store_draft_upload => {
                    let _ = log_tx.send("Computer upload disabled; stage skipped.".into());
                    Ok(())
                }
                Stage::Publish => {
                    let prompt =
                        computer::policy_and_store_prompt(&cfg, &project_path, &product_brief);
                    computer::run_agent_task(&cfg, &project_path, &prompt, log_tx.clone())
                        .map(|_| ())
                        .map_err(|e| e.to_string())
                }
                Stage::Quality if quality_computer => {
                    let prompt = stage_prompt(stage, &product_brief, &project_path);
                    let _ = log_tx.send(
                    "Quality repair is using the AI controller with CUA Driver for runtime checks."
                        .into(),
                );
                    computer::run_agent_task(&cfg, &project_path, &prompt, log_tx.clone())
                        .map(|_| ())
                        .map_err(|e| e.to_string())
                }
                _ => {
                    let mut prompt = stage_prompt(stage, &product_brief, &project_path);
                    if stage == Stage::Store && cfg.store_draft_upload {
                        prompt.push_str(
                            "\n\nSTORE DRAFT UPLOAD IS ENABLED FOR THIS APPFORGE CONFIG. You MUST write .appforge/store-upload-request.conf exactly as described above so the human can inspect and approve the external draft actions before Publish. Missing this request blocks Publish.",
                        );
                    }
                    provider::run_task(
                        &provider_id,
                        &project_path,
                        &prompt,
                        cfg.auto_mode,
                        cfg.strict_subscription_auth,
                        log_tx.clone(),
                    )
                    .map_err(|e| e.to_string())
                }
            }
        };
        drop(log_tx);
        let _ = forwarder.join();
        let mut result = result;
        if stage == Stage::Publish && (cfg.notion_enabled || cfg.store_draft_upload) {
            if let Err(err) = project::consume_publish_approval(&project_path) {
                result = Err(format!("publish approval could not be consumed: {err}"));
            }
        }
        if result.is_ok() {
            if let Some(gate) = gate {
                result = verify_decision(&project_path, gate).map_err(|e| e.to_string());
            }
        }
        let note = match &result {
            Ok(_) => "agent stage completed".to_string(),
            Err(err) => err.clone(),
        };
        if let Err(err) = project::mark_stage(&project_path, stage.id(), result.is_ok(), &note) {
            result = Err(format!("stage status could not be saved: {err}"));
        }
        let _ = tx.send(WorkerEvent::Completed {
            stage,
            provider: provider_id,
            result,
        });
    });
}

fn valid_notion_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let host = rest.split('/').next().unwrap_or("");
    matches!(host, "notion.so" | "www.notion.so" | "notion.site") || host.ends_with(".notion.site")
}

fn verify_decision(project: &Path, gate: &str) -> io::Result<()> {
    let decision = std::fs::read_to_string(project.join(format!(".appforge/{gate}-decision")))?;
    let report = if gate == "quality" {
        "docs/04-quality.md"
    } else {
        "docs/05-qa.md"
    };
    if decision.trim() != "PASS"
        || std::fs::read_to_string(project.join(report))?
            .trim()
            .is_empty()
    {
        return Err(io::Error::other(format!(
            "{gate} gate BLOCKED or missing evidence"
        )));
    }
    Ok(())
}

fn verify_gates(project: &Path) -> io::Result<()> {
    verify_decision(project, "quality")?;
    verify_decision(project, "qa")
}

fn stage_prompt(stage: Stage, brief: &str, project: &Path) -> String {
    let common = format!(
        "You are working inside {}. Read AGENTS.md and all existing docs before acting. Product brief: {}. Keep work production-oriented, cross-platform, measurable, and reproducible. Never inspect or extract OAuth tokens, browser cookies, keychains, or another tool's credential files. Use existing authenticated CLIs only. Do not publish anything, upload store artifacts, change account settings, or submit a release in this stage; only the separately configured Publish stage can publish policy pages or upload approved drafts.",
        project.display(),
        brief
    );

    let task = match stage {
        Stage::Plan => {
            "Act as the PRIMARY ORCHESTRATOR. Do not implement the app yet. Define product scope, core loop, MVP/non-goals, monetization assumptions if relevant, target users, platform constraints, acceptance criteria, and task DAG. Write docs/01-product.md. Also decide whether Expo/React Native + TypeScript is sufficient or document why another engine is necessary in docs/03-architecture.md."
        }
        Stage::Design => {
            "Act as the DESIGN SUB-AGENT. Read docs/01-product.md and docs/aside/design.md if present. Produce a concrete mobile-first game/app UX: screen map, controls, feedback, states, typography/color guidance, asset list, accessibility, onboarding, retention loop, and screenshot plan. Write docs/02-design.md. You may add lightweight wireframe/spec files, but do not replace the product scope."
        }
        Stage::Build => {
            "Act as the DEVELOPMENT SUB-AGENT. Implement the MVP described by docs/01-product.md and docs/02-design.md. Prefer Expo + React Native + TypeScript for casual mobile games unless docs/03-architecture.md justifies another stack. Add real source code, tests, lint/typecheck/build scripts, app identifiers/placeholders, and developer README instructions. Keep Android and iOS buildability. Update docs/03-architecture.md with actual decisions. Do not claim success without running available checks."
        }
        Stage::Quality => {
            r#"Act as the FUNCTIONAL + PERFORMANCE REPAIR AGENT. Your job is not to write a favorable review; find weak functionality and measurable performance risks, then fix them.

1. Detect the actual stack and package manager from lockfiles/config.
2. Run the strongest available lint, typecheck, unit/integration tests, production build/export, framework doctor, and static analysis without weakening existing gates.
3. Exercise the primary user/game loop. If a runnable simulator/emulator/browser target and Computer Use are available, launch it and verify critical interactions, navigation, persistence, error/offline states, touch targets, loading/empty states, and restart behavior.
4. Measure what the stack makes practical: startup/build/export time, bundle/asset size, long-running tasks, excessive rerenders, unbounded lists, image/media waste, synchronous storage/network bottlenecks, memory/resource leaks, and obvious frame-rate risks. Do not invent benchmark numbers when tooling cannot measure them.
5. Prioritize P0/P1 functional defects and user-visible performance regressions. Fix verified issues in the code, then rerun the relevant checks.
6. Do not replace real checks with mocks merely to pass. Do not delete features to improve a metric.
7. Write docs/04-quality.md containing before/after evidence, commands or runtime checks performed, fixes made, unresolved risks, and a clear PASS/BLOCKED release decision."#
        }
        Stage::Qa => {
            "Act as the PRIMARY REVIEWER after the quality-repair stage. Inspect the complete implementation and git diff. Run available tests/typechecks/build checks again. Fix correctness, state-management, UX-blocking, security/privacy, accessibility, and mobile compatibility issues you can verify. Recheck issues recorded in docs/04-quality.md. Write docs/05-qa.md with commands run, results, remaining risks, and concrete release blockers. Do not lower quality gates just to make checks green."
        }
        Stage::Store => {
            r#"Act as the STORE/POLICY SUB-AGENT. Read the actual app implementation, dependencies, permissions, network/storage/auth/analytics/ad behavior, docs/04-quality.md, docs/05-qa.md, and docs/aside/store.md if present.

Prepare docs/06-store.md with exact Google Play and Apple App Store listing copy, package/bundle identifiers, category suggestions, privacy/data disclosure checklist, permissions rationale, icon/screenshot inventory, age/content rating inputs, build artifact locations, support fields, and manual approval points.

Create docs/policies/privacy-policy.md based on the app's VERIFIED data practices; do not claim that data is not collected if the code or third-party SDKs collect it. Also create docs/policies/terms.md and docs/policies/support-and-data-deletion.md when relevant.

Use PUBLIC_PRIVACY_POLICY_URL, PUBLIC_TERMS_URL, and PUBLIC_SUPPORT_URL placeholders in docs/06-store.md so the next Computer Use stage can replace them with published Notion URLs.

If store draft upload may be used, write .appforge/store-upload-request.conf with these newline-delimited fields:
app_identifier=<exact Android package and/or Apple bundle identifier, with platform labels>
allowed_actions=metadata,policy_urls,screenshots,build_upload
artifact=<relative path>|sha256=<actual SHA-256> for each artifact that exists
approved=false
Do not include an artifact that was not actually built and hashed.

Do not submit to a store, create irreversible identifiers, accept legal agreements, or invent signing credentials."#
        }
        Stage::Publish => {
            "Computer-use publication is handled by the dedicated CUA stage."
        }
        Stage::Release => {
            "Act as the PRIMARY RELEASE ORCHESTRATOR. Read docs/04-quality.md, docs/05-qa.md, docs/06-store.md, and docs/07-publish.md when present. Do not declare release-ready if Quality or QA says BLOCKED. Verify versioning, CI, build commands, signing placeholders, changelog/release notes, reproducible artifacts, and store metadata consistency. Write docs/08-release.md. GitHub Release automation may be configured, but App Store / Play Store final review submission and production rollout remain explicit human approval steps."
        }
    };
    let gate = match stage {
        Stage::Quality => "quality",
        Stage::Qa => "qa",
        _ => "",
    };
    let decision = if gate.is_empty() {
        String::new()
    } else {
        format!("\nWrite .appforge/{gate}-decision containing exactly PASS or BLOCKED on one line. PASS requires actual successful relevant checks, no unresolved release blockers, and a durable docs report. Missing tools or failed required checks mean BLOCKED. Do not reuse a prior decision.")
    };
    format!("{common}\n\nSTAGE TASK:\n{task}{decision}")
}

fn truncate(value: &str, max: usize) -> String {
    let compact = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() <= max {
        compact
    } else {
        compact
            .chars()
            .take(max.saturating_sub(1))
            .collect::<String>()
            + "…"
    }
}

pub fn run_stages(
    cfg: Config,
    project: PathBuf,
    brief: String,
    stages: &[Stage],
) -> io::Result<()> {
    for stage in stages {
        let (tx, rx) = mpsc::channel();
        spawn_stage(cfg.clone(), project.clone(), brief.clone(), *stage, tx);
        let mut completed = None;
        while let Ok(event) = rx.recv() {
            match event {
                WorkerEvent::Log(line) => println!("{line}"),
                WorkerEvent::Completed { result, .. } => {
                    completed = Some(result);
                    break;
                }
            }
        }
        match completed {
            Some(Ok(())) => {}
            Some(Err(err)) => return Err(io::Error::other(err)),
            None => return Err(io::Error::other("worker exited without completion event")),
        }
    }
    Ok(())
}

pub fn run_all(cfg: Config, project: PathBuf, brief: String) -> io::Result<()> {
    run_stages(cfg, project, brief, Stage::all())
}

pub fn run_repair(cfg: Config, project: PathBuf, brief: String) -> io::Result<()> {
    run_stages(cfg, project, brief, Stage::repair())
}

pub fn run_publish(cfg: Config, project: PathBuf, brief: String) -> io::Result<()> {
    run_stages(cfg, project, brief, &[Stage::Publish, Stage::Release])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> Config {
        Config {
            primary: "codex".into(),
            secondary: vec!["claude".into(), "cursor".into()],
            enabled: vec!["codex".into(), "claude".into(), "cursor".into()],
            projects_dir: PathBuf::from("/tmp"),
            auto_mode: true,
            aside_enabled: false,
            strict_subscription_auth: true,
            notion_enabled: false,
            notion_target_kind: "page".into(),
            notion_target_url: String::new(),
            notion_publish_public: true,
            computer_backend: "auto".into(),
            store_draft_upload: false,
        }
    }

    #[test]
    fn routes_primary_and_secondary_roles() {
        let cfg = cfg();
        assert_eq!(provider_for(&cfg, Stage::Plan), "codex");
        assert_eq!(provider_for(&cfg, Stage::Design), "claude");
        assert_eq!(provider_for(&cfg, Stage::Build), "cursor");
        assert_eq!(provider_for(&cfg, Stage::Quality), "codex");
        assert_eq!(provider_for(&cfg, Stage::Qa), "codex");
        assert_eq!(provider_for(&cfg, Stage::Store), "claude");
    }

    #[test]
    fn notion_target_validation() {
        assert!(valid_notion_url("https://www.notion.so/page"));
        assert!(!valid_notion_url("https://notion.so.evil.test/page"));
        assert!(!valid_notion_url("https://evil@notion.so/page"));
    }

    #[test]
    fn repair_stages_are_quality_then_qa() {
        assert_eq!(Stage::repair(), &[Stage::Quality, Stage::Qa]);
    }
}
