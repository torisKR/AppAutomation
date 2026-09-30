mod aside;
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
        }
        Err(err) => println!("config status: {err}"),
    }
    println!(
        "aside binary: {}",
        if aside::available() { "yes" } else { "no" }
    );
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
  appforge doctor              Check CLIs, auth, Aside, and config
  appforge providers           List supported provider status
  appforge login <provider>    Run the provider's official login flow
  appforge create <brief>      Create a generated app workspace only
  appforge new <brief>         Create a workspace and run the full pipeline
  appforge run [project-dir]   Resume/run all stages for an existing workspace
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
  Product plan → Design → Development → QA → Store readiness → Release

AUTH POLICY
  Codex, Claude Code, Cursor, and Antigravity are invoked through their official
  authenticated CLIs. AppForge removes ambient API-key environment variables for
  those providers so an existing subscription/OAuth session is preferred.
  OpenCode Go is a subscription but its official connection method is a key, so
  strict_subscription_auth=true excludes it from automatic execution."
    );
}
