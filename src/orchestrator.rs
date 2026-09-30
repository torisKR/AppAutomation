use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::thread;

use crate::aside;
use crate::config::Config;
use crate::project;
use crate::provider;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Plan,
    Design,
    Build,
    Qa,
    Store,
    Release,
}

impl Stage {
    pub fn all() -> &'static [Stage] {
        &[
            Stage::Plan,
            Stage::Design,
            Stage::Build,
            Stage::Qa,
            Stage::Store,
            Stage::Release,
        ]
    }

    pub fn id(self) -> &'static str {
        match self {
            Stage::Plan => "plan",
            Stage::Design => "design",
            Stage::Build => "build",
            Stage::Qa => "qa",
            Stage::Store => "store",
            Stage::Release => "release",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Stage::Plan => "01 Product plan",
            Stage::Design => "02 UX / game design",
            Stage::Build => "03 Development",
            Stage::Qa => "04 QA & review",
            Stage::Store => "05 Store readiness",
            Stage::Release => "06 Release",
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
        Stage::Plan | Stage::Qa | Stage::Release => cfg.primary.clone(),
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

pub fn spawn_stage(
    cfg: Config,
    project_path: PathBuf,
    product_brief: String,
    stage: Stage,
    tx: Sender<WorkerEvent>,
) {
    thread::spawn(move || {
        let provider_id = provider_for(&cfg, stage);
        let _ = tx.send(WorkerEvent::Log(format!(
            "{} → {}",
            stage.title(),
            provider::label(&provider_id)
        )));

        if cfg.aside_enabled {
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

        let prompt = stage_prompt(stage, &product_brief, &project_path);
        let (log_tx, log_rx) = mpsc::channel::<String>();
        let forward_tx = tx.clone();
        let forwarder = thread::spawn(move || {
            while let Ok(line) = log_rx.recv() {
                let _ = forward_tx.send(WorkerEvent::Log(line));
            }
        });

        let result = provider::run_task(
            &provider_id,
            &project_path,
            &prompt,
            cfg.auto_mode,
            cfg.strict_subscription_auth,
            log_tx,
        )
        .map_err(|e| e.to_string());

        let _ = forwarder.join();
        let note = match &result {
            Ok(_) => "agent stage completed".to_string(),
            Err(err) => err.clone(),
        };
        let _ = project::mark_stage(&project_path, stage.id(), result.is_ok(), &note);
        let _ = tx.send(WorkerEvent::Completed {
            stage,
            provider: provider_id,
            result,
        });
    });
}

fn stage_prompt(stage: Stage, brief: &str, project: &Path) -> String {
    let common = format!(
        "You are working inside {}. Read AGENTS.md and all existing docs before acting. Product brief: {}. Keep work production-oriented, cross-platform, and reproducible. Never inspect or extract OAuth tokens, browser cookies, keychains, or another tool's credential files. Use existing authenticated CLIs only.",
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
        Stage::Qa => {
            "Act as the PRIMARY REVIEWER. Inspect the complete implementation and git diff. Run available tests/typechecks/build checks. Fix correctness, state-management, UX-blocking, security/privacy, and mobile compatibility issues you can verify. Write docs/04-qa.md with commands run, results, remaining risks, and concrete release blockers. Do not lower quality gates just to make checks green."
        }
        Stage::Store => {
            "Act as the STORE/PRODUCT SUB-AGENT. Prepare docs/05-store.md with Google Play and Apple App Store listing copy, category suggestions, privacy/data disclosure checklist, permissions rationale, icon/screenshot inventory, age/content rating inputs, support/privacy URL placeholders, and manual approval points. Use Aside notes if present. You may prepare metadata files but MUST NOT submit or accept legal agreements."
        }
        Stage::Release => {
            "Act as the PRIMARY RELEASE ORCHESTRATOR. Make the repository release-ready: verify versioning, CI, build commands, signing placeholders, changelog/release notes, and GitHub Actions needed for reproducible artifacts. Write docs/06-release.md. GitHub Release automation may be configured, but App Store / Play Store irreversible submission must remain a human approval step."
        }
    };
    format!("{common}\n\nSTAGE TASK:\n{task}")
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

pub fn run_all(cfg: Config, project: PathBuf, brief: String) -> io::Result<()> {
    for stage in Stage::all() {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secondary_handles_design_and_build() {
        let cfg = Config {
            primary: "codex".into(),
            secondary: vec!["claude".into(), "cursor".into()],
            enabled: vec!["codex".into(), "claude".into(), "cursor".into()],
            projects_dir: PathBuf::from("/tmp"),
            auto_mode: true,
            aside_enabled: false,
            strict_subscription_auth: true,
        };
        assert_eq!(provider_for(&cfg, Stage::Plan), "codex");
        assert_eq!(provider_for(&cfg, Stage::Design), "claude");
        assert_eq!(provider_for(&cfg, Stage::Build), "cursor");
        assert_eq!(provider_for(&cfg, Stage::Qa), "codex");
    }
}
