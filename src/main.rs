mod aside;
mod computer;
mod config;
mod orchestrator;
mod project;
mod provider;
mod terminal;
mod tui;

use std::env;
use std::io;
use std::path::PathBuf;

fn main() {
    if let Err(err) = run() {
        eprintln!("appforge: {err}");
        std::process::exit(1);
    }
}

fn run() -> io::Result<()> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.is_empty() {
        let cfg = config::ensure()?;
        return tui::run(cfg);
    }

    match args[0].as_str() {
        "tui" => {
            let cfg = config::ensure()?;
            tui::run(cfg)
        }
        "setup" => {
            let cfg = config::onboard()?;
            println!("primary: {}", provider::label(&cfg.primary));
            println!(
                "secondary: {}",
                cfg.secondary
                    .iter()
                    .map(|id| provider::label(id))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            Ok(())
        }
        "doctor" => doctor(),
        "computer" => match args.get(1).map(String::as_str).unwrap_or("status") {
            "status" => {
                println!("{}", computer::status().summary());
                Ok(())
            }
            "setup" => computer::setup_interactive(),
            other => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("unknown computer command: {other}. Use status or setup."),
            )),
        },
        "providers" => {
            println!(
                "{:<12} {:<5} {:<18} {:<36} BINARY",
                "PROVIDER", "INST", "AUTH", "STATUS"
            );
            for info in provider::detected() {
                println!("{}", provider::doctor_line(&info));
            }
            Ok(())
        }
        "login" => {
            let Some(id) = args.get(1) else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "usage: appforge login <codex|claude|cursor|antigravity|opencode>",
                ));
            };
            provider::login(id)
        }
        "create" => {
            let cfg = config::ensure()?;
            let brief = args[1..].join(" ");
            if brief.trim().is_empty() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "usage: appforge create <game/app brief>",
                ));
            }
            let path = project::create(&cfg.projects_dir, &brief)?;
            println!("{}", path.display());
            Ok(())
        }
        "new" => {
            let cfg = config::ensure()?;
            let brief = args[1..].join(" ");
            if brief.trim().is_empty() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "usage: appforge new <game/app brief>",
                ));
            }
            let path = project::create(&cfg.projects_dir, &brief)?;
            println!("project: {}", path.display());
            orchestrator::run_all(cfg, path, brief)
        }
        "run" => {
            let cfg = config::ensure()?;
            let path = args
                .get(1)
                .map(PathBuf::from)
                .unwrap_or(env::current_dir()?);
            let brief = project::load_brief(&path)?;
            orchestrator::run_all(cfg, path, brief)
        }
        "repair" => {
            let cfg = config::ensure()?;
            let path = args
                .get(1)
                .map(PathBuf::from)
                .unwrap_or(env::current_dir()?);
            let brief = project::load_brief(&path)?;
            println!("repair: {}", path.display());
            orchestrator::run_repair(cfg, path, brief)
        }
        "approve-publish" => {
            let cfg = config::ensure()?;
            let path = args
                .get(1)
                .map(PathBuf::from)
                .unwrap_or(env::current_dir()?);
            let _ = project::load_brief(&path)?;
            project::approve_publish(&path, &cfg)
        }
        "publish" => {
            let cfg = config::ensure()?;
            let path = args
                .get(1)
                .map(PathBuf::from)
                .unwrap_or(env::current_dir()?);
            let brief = project::load_brief(&path)?;
            println!("publish: {}", path.display());
            orchestrator::run_publish(cfg, path, brief)
        }
        "repair-all" => {
            let cfg = config::ensure()?;
            let root = args
                .get(1)
                .map(PathBuf::from)
                .unwrap_or_else(|| cfg.projects_dir.clone());
            let projects = project::discover(&root)?;
            if projects.is_empty() {
                println!("No AppForge projects found under {}", root.display());
                return Ok(());
            }
            println!("Found {} AppForge project(s).", projects.len());
            let mut failures = Vec::new();
            for path in projects {
                let brief = match project::load_brief(&path) {
                    Ok(brief) => brief,
                    Err(err) => {
                        failures.push(format!("{}: {err}", path.display()));
                        continue;
                    }
                };
                println!("\n=== repair {} ===", path.display());
                if let Err(err) = orchestrator::run_repair(cfg.clone(), path.clone(), brief) {
                    failures.push(format!("{}: {err}", path.display()));
                }
            }
            if failures.is_empty() {
                Ok(())
            } else {
                Err(io::Error::other(failures.join("\n")))
            }
        }
        "config" => {
            println!("{}", config::config_path().display());
            Ok(())
        }
        "version" | "--version" | "-V" => {
            println!("appforge {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        other => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("unknown command: {other}. Run appforge help."),
        )),
    }
}

fn doctor() -> io::Result<()> {
    println!("AppForge doctor");
    println!("===============");
    println!("config: {}", config::config_path().display());
    match config::Config::load() {
        Ok(cfg) => {
            println!("primary: {}", provider::label(&cfg.primary));
            println!("projects: {}", cfg.projects_dir.display());
            println!("auto: {}", cfg.auto_mode);
            println!("aside: {}", cfg.aside_enabled);
            println!("strict subscription auth: {}", cfg.strict_subscription_auth);
            println!("notion policy publish: {}", cfg.notion_enabled);
            if cfg.notion_enabled {
                println!(
                    "notion target: {} {}",
                    cfg.notion_target_kind, cfg.notion_target_url
                );
                println!("notion public link: {}", cfg.notion_publish_public);
            }
            println!("computer backend: {}", cfg.computer_backend);
            println!("store draft upload: {}", cfg.store_draft_upload);
        }
        Err(err) => println!("config status: {err}"),
    }
    println!(
        "aside binary: {}",
        if aside::available() { "yes" } else { "no" }
    );
    println!("computer use: {}", computer::status().summary());
    println!();
    println!(
        "{:<12} {:<5} {:<18} {:<36} BINARY",
        "PROVIDER", "INST", "AUTH", "STATUS"
    );
    for info in provider::detected() {
        println!("{}", provider::doctor_line(&info));
    }
    Ok(())
}

fn print_help() {
    println!(
        "AppForge — subscription-first multi-agent app factory

USAGE
  appforge                     Open the split-screen TUI
  appforge setup               Run first-run provider/project setup
  appforge doctor              Check CLIs, auth, Aside, CUA, Notion/store config
  appforge computer status     Check CUA Driver/controller readiness
  appforge computer setup      Install/configure CUA Driver when needed
  appforge providers           List supported provider status
  appforge login <provider>    Run the provider's official login flow
  appforge create <brief>      Create a generated app workspace only
  appforge new <brief>         Create a workspace and run the full pipeline
  appforge run [project-dir]   Resume/run all stages for an existing workspace
  appforge repair [project]    Find/fix functional and performance weaknesses
  appforge repair-all [root]   Repair every generated AppForge project under a root
  appforge approve-publish [project]
                               Review/approve one Notion/store draft publish attempt
  appforge publish [project]   Run approved Notion/store draft upload then release gate
  appforge config              Print config path
  appforge version             Print version

TUI COMMANDS
  <brief>      Create a new app/game project
  auto         Continue stages automatically
  manual       Pause before the next stage
  run          Run the next stage manually
  Ctrl+C       Switch to manual mode
  quit         Exit when no worker is active

PIPELINE
  Plan → Design → Development → Functional/performance repair → QA
  → Store/policies → Notion/store draft upload → Release

AUTH POLICY
  Codex, Claude Code, Cursor, and Antigravity are invoked through their official
  authenticated CLIs. AppForge removes ambient API-key environment variables for
  those providers so an existing subscription/OAuth session is preferred.
  OpenCode Go is a subscription but its official connection method is a key, so
  strict_subscription_auth=true excludes it from automatic execution."
    );
}
