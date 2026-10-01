use std::env;
use std::io::{self, BufRead, BufReader, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::Sender;
use std::thread;

use crate::config::Config;
use crate::provider;

#[derive(Clone, Debug)]
pub struct ComputerStatus {
    pub cua_installed: bool,
    pub daemon_running: bool,
    pub permissions_ready: bool,
    pub codex_available: bool,
    pub claude_available: bool,
}

impl ComputerStatus {
    pub fn summary(&self) -> String {
        format!(
            "CUA={} daemon={} permissions={} codex={} claude={}",
            yes(self.cua_installed),
            yes(self.daemon_running),
            yes(self.permissions_ready),
            yes(self.codex_available),
            yes(self.claude_available),
        )
    }

    pub fn ready(&self) -> bool {
        self.cua_installed && self.daemon_running && self.permissions_ready
    }
}

fn yes(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

fn driver_binary() -> Option<PathBuf> {
    let mut dirs = env::var_os("PATH")
        .map(|p| env::split_paths(&p).collect::<Vec<_>>())
        .unwrap_or_default();
    if let Some(home) = env::var_os("HOME") {
        dirs.push(PathBuf::from(home).join(".local/bin"));
    }
    dirs.into_iter()
        .map(|dir| {
            dir.join(if cfg!(windows) {
                "cua-driver.exe"
            } else {
                "cua-driver"
            })
        })
        .find(|p| provider::is_executable(p))
        .and_then(|p| p.canonicalize().ok())
}

fn has_binary(_: &str) -> bool {
    driver_binary().is_some()
}

fn daemon_ready(ok: bool, text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    ok && text
        .lines()
        .any(|line| line.trim() == "cua driver daemon is running")
}

fn permissions_ready(ok: bool, text: &str) -> bool {
    ok && ["Accessibility:", "Screen Recording:"].iter().all(|name| {
        text.lines().any(|line| {
            line.trim().starts_with(name) && line.contains('✅') && !line.contains('❌')
        })
    })
}

fn output(name: &str, args: &[&str]) -> io::Result<(bool, String)> {
    let mut command = Command::new(driver_binary().unwrap_or_else(|| PathBuf::from(name)));
    command.args(args);
    let (status, stdout, stderr) = provider::probe_output(command)?;
    let mut text = String::from_utf8_lossy(&stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&stderr).trim().to_string();
    if !stderr.is_empty() {
        if !text.is_empty() {
            text.push_str(" · ");
        }
        text.push_str(&stderr);
    }
    Ok((status.success(), text))
}

fn daemon_is_running() -> bool {
    output("cua-driver", &["status"])
        .map(|(ok, text)| daemon_ready(ok, &text))
        .unwrap_or(false)
}

fn permissions_are_ready() -> bool {
    output("cua-driver", &["permissions", "status"])
        .map(|(ok, text)| permissions_ready(ok, &text))
        .unwrap_or(false)
}

pub fn status() -> ComputerStatus {
    let cua_installed = has_binary("cua-driver");
    let daemon_running = cua_installed && daemon_is_running();
    let permissions_ready = cua_installed && permissions_are_ready();

    ComputerStatus {
        cua_installed,
        daemon_running,
        permissions_ready,
        codex_available: provider::find_binary("codex").is_some(),
        claude_available: provider::find_binary("claude").is_some(),
    }
}

pub fn install_cua() -> io::Result<()> {
    if has_binary("cua-driver") {
        return Ok(());
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        let script = Command::new("curl")
            .args([
                "--fail",
                "--show-error",
                "--location",
                "--proto",
                "=https",
                "--max-time",
                "120",
                "https://cua.ai/driver/install.sh",
            ])
            .output()?;
        if !script.status.success() || script.stdout.is_empty() {
            return Err(io::Error::other("CUA installer download failed"));
        }
        let mut child = Command::new("/bin/bash")
            .stdin(Stdio::piped())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()?;
        child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("installer stdin missing"))?
            .write_all(&script.stdout)?;
        let status = child.wait()?;
        if !status.success() {
            return Err(io::Error::other(format!(
                "CUA Driver installer exited with {status}"
            )));
        }
    }

    #[cfg(target_os = "windows")]
    {
        let status = Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                "$ErrorActionPreference = 'Stop'; try { irm https://cua.ai/driver/install.ps1 | iex } catch { Write-Error $_; exit 1 }",
            ])
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()?;
        if !status.success() {
            return Err(io::Error::other(format!(
                "CUA Driver installer exited with {status}"
            )));
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    return Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "automatic CUA Driver installation is not supported on this OS",
    ));

    if !has_binary("cua-driver") {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "CUA Driver installer finished but cua-driver is not on PATH",
        ));
    }
    Ok(())
}

fn start_daemon() -> io::Result<()> {
    if !has_binary("cua-driver") {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "cua-driver is not installed",
        ));
    }
    if daemon_is_running() {
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        let launch = Command::new("open")
            .args(["-n", "-g", "-a", "CuaDriver", "--args", "serve"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()?;
        if !launch.success() {
            return Err(io::Error::other(format!(
                "failed to start CUA Driver app daemon: {launch}"
            )));
        }
    }

    #[cfg(target_os = "linux")]
    {
        let driver = driver_binary().ok_or_else(|| io::Error::other("CUA Driver missing"))?;
        Command::new(&driver)
            .arg("serve")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
    }

    #[cfg(target_os = "windows")]
    {
        let driver = driver_binary().ok_or_else(|| io::Error::other("CUA Driver missing"))?;
        let kicked = Command::new(&driver)
            .args(["autostart", "kick"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()?;
        if !kicked.success() {
            Command::new(&driver)
                .arg("serve")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?;
        }
    }

    for _ in 0..20 {
        if daemon_is_running() {
            return Ok(());
        }
        thread::sleep(std::time::Duration::from_millis(250));
    }

    Err(io::Error::other(
        "CUA Driver daemon did not become ready; run cua-driver doctor",
    ))
}

pub fn grant_permissions() -> io::Result<()> {
    if !has_binary("cua-driver") {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "cua-driver is not installed",
        ));
    }
    start_daemon()?;
    let status =
        Command::new(driver_binary().ok_or_else(|| io::Error::other("CUA Driver missing"))?)
            .args(["permissions", "grant"])
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()?;
    if status.success() && self::status().permissions_ready {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "CUA permissions remain unverified; grant exited with {status}"
        )))
    }
}

pub fn setup_interactive() -> io::Result<()> {
    if !io::stdin().is_terminal() {
        return Err(io::Error::other(
            "computer setup requires an interactive terminal",
        ));
    }
    let current = status();
    println!("Computer Use: {}", current.summary());
    if !current.cua_installed {
        print!("CUA Driver가 없습니다. 공식 설치 스크립트로 설치할까요? [Y/n]: ");
        io::stdout().flush()?;
        let mut answer = String::new();
        io::stdin().read_line(&mut answer)?;
        if !matches!(answer.trim().to_ascii_lowercase().as_str(), "n" | "no") {
            install_cua()?;
        }
    }

    if status().cua_installed {
        start_daemon()?;
    }

    let current = status();
    if current.cua_installed && !current.permissions_ready {
        print!("CUA Driver OS 권한을 설정할까요? [Y/n]: ");
        io::stdout().flush()?;
        let mut answer = String::new();
        io::stdin().read_line(&mut answer)?;
        if !matches!(answer.trim().to_ascii_lowercase().as_str(), "n" | "no") {
            grant_permissions()?;
        }
    }

    println!("Computer Use: {}", status().summary());
    Ok(())
}

pub fn controller(cfg: &Config) -> Option<String> {
    let allowed = |id: &str| cfg.enabled.iter().any(|enabled| enabled == id);
    let installed = |id: &str| provider::find_binary(id).is_some();

    match cfg.computer_backend.as_str() {
        "codex" if allowed("codex") && installed("codex") => Some("codex".into()),
        "claude" if allowed("claude") && installed("claude") => Some("claude".into()),
        "auto" => {
            if matches!(cfg.primary.as_str(), "codex" | "claude")
                && allowed(&cfg.primary)
                && installed(&cfg.primary)
            {
                return Some(cfg.primary.clone());
            }
            ["codex", "claude"]
                .into_iter()
                .find(|id| allowed(id) && installed(id))
                .map(str::to_string)
        }
        _ => None,
    }
}

fn codex_command(binary: &Path, cwd: &Path, prompt: &str, _auto: bool) -> Command {
    let mut cmd = Command::new(binary);
    let driver = driver_binary().unwrap_or_else(|| PathBuf::from("cua-driver"));
    cmd.args(["exec", "-m", "gpt-5.6-sol"]);
    // Computer-use is already bounded by AppForge's explicit task contract,
    // the CUA Driver standard permission mode, and (for external writes) the
    // one-attempt approve-publish gate. Codex needs its automatic reviewer to
    // authorize GUI tool calls such as launch_app/click. On current Codex,
    // --approve-for-me already selects workspace-write sandboxing and cannot be
    // combined with an explicit --sandbox flag.
    cmd.arg("--approve-for-me");
    cmd.current_dir(cwd)
        .stdin(Stdio::null())
        .args([
            "--ignore-user-config",
            "--skip-git-repo-check",
            "--disable",
            "plugins",
            "--disable",
            "apps",
            "--disable",
            "skill_search",
            "--enable",
            "skip_host_skill_discovery",
            "--json",
            "-c",
            "forced_login_method=\"chatgpt\"",
            "-c",
            "model_provider=\"openai\"",
            "-c",
            "model_reasoning_effort=\"medium\"",
            "-c",
            "suppress_unstable_features_warning=true",
            "-c",
            &format!(
                "mcp_servers.computer.command={:?}",
                driver.to_string_lossy()
            ),
            "-c",
            "mcp_servers.computer.args=[\"mcp\"]",
            "-C",
        ])
        .arg(cwd)
        .arg("--")
        .arg(prompt);
    provider::subscription_environment(&mut cmd, "codex");
    cmd
}

fn claude_command(binary: &Path, cwd: &Path, prompt: &str, auto: bool) -> Command {
    let mut cmd = Command::new(binary);
    let driver = driver_binary().unwrap_or_else(|| PathBuf::from("cua-driver"));
    let mcp = format!(
        r#"{{"mcpServers":{{"computer":{{"command":{:?},"args":["mcp"]}}}}}}"#,
        driver.to_string_lossy()
    );
    cmd.current_dir(cwd)
        .stdin(Stdio::null())
        .args([
            "-p",
            "--verbose",
            "--output-format",
            "stream-json",
            "--setting-sources",
            "",
            "--permission-mode",
            if auto { "auto" } else { "acceptEdits" },
            "--permission-prompts",
            "none",
            "--mcp-config",
            &mcp,
            "--strict-mcp-config",
            "--",
        ])
        .arg(prompt);
    provider::subscription_environment(&mut cmd, "claude");
    cmd
}

fn stream_command(
    mut cmd: Command,
    label: &str,
    tx: Sender<String>,
    timeout: Option<std::time::Duration>,
) -> io::Result<()> {
    let mut child = cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let tx_out = tx.clone();
    let out = thread::spawn(move || {
        if let Some(stdout) = stdout {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                let _ = tx_out.send(format!("│ {line}"));
            }
        }
    });

    let tx_err = tx.clone();
    let err = thread::spawn(move || {
        if let Some(stderr) = stderr {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                let _ = tx_err.send(format!("! {line}"));
            }
        }
    });

    let started = std::time::Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if timeout.is_some_and(|limit| started.elapsed() >= limit) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = out.join();
            let _ = err.join();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("{label} computer task exceeded its time limit"),
            ));
        }
        thread::sleep(std::time::Duration::from_millis(250));
    };

    let _ = out.join();
    let _ = err.join();

    if status.success() {
        let _ = tx.send(format!("{label} ✓ computer task completed"));
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "{label} computer task exited with {status}"
        )))
    }
}

pub fn run_agent_task(
    cfg: &Config,
    cwd: &Path,
    prompt: &str,
    tx: Sender<String>,
) -> io::Result<String> {
    if has_binary("cua-driver") && !daemon_is_running() {
        start_daemon()?;
    }
    let current = status();
    if !current.ready() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "Computer Use is not ready: {}. Run appforge computer setup.",
                current.summary()
            ),
        ));
    }

    let controller = controller(cfg).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "No enabled Codex/Claude controller is available for Computer Use",
        )
    })?;
    let binary = provider::find_binary(&controller)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "controller CLI missing"))?;

    let cwd = cwd.canonicalize()?;
    let safety = r#"
COMPUTER-USE CONTRACT:
- This AppForge task is self-contained. Do not inspect or invoke unrelated global skills, gstack, ~/.agents, ~/.claude, or ~/.codex instruction frameworks; use only this project's AGENTS.md, project files, the supplied task, and the computer MCP.
- Use only the exact Notion/store resources named in this task. Runtime quality checks grant NO Notion/store write authority. Never change account settings, global MCP settings, pricing, sharing of existing resources, or legal agreements.
- Use the CUA Driver MCP 'computer' server for browser/native GUI interaction.
- Never inspect or copy passwords, MFA secrets, cookies, OAuth tokens, keychains, or unrelated tabs.
- Re-snapshot after navigation and verify the observable state after each consequential UI action.
- You may create/update draft metadata, upload existing build artifacts, and publish the specifically requested policy pages when the task says so.
- Never click final review submission, production rollout, release-to-users, purchase, legal agreement acceptance, account deletion, or destructive unrelated actions.
- If login/MFA/passkey is required, stop and record the blocker for the human.
- Write a durable result file requested by the task before exiting.
"#;
    let full_prompt = format!("{safety}\n\nTASK:\n{prompt}");

    let _ = tx.send(format!(
        "{} + CUA Driver ▶ browser/computer task",
        provider::label(&controller)
    ));
    let command = match controller.as_str() {
        "codex" => codex_command(&binary, &cwd, &full_prompt, cfg.auto_mode),
        "claude" => claude_command(&binary, &cwd, &full_prompt, cfg.auto_mode),
        _ => unreachable!(),
    };
    stream_command(command, provider::label(&controller), tx, None)?;
    Ok(controller)
}

pub fn run_agent_task_bounded(
    cfg: &Config,
    cwd: &Path,
    prompt: &str,
    tx: Sender<String>,
    timeout: std::time::Duration,
) -> io::Result<String> {
    if has_binary("cua-driver") && !daemon_is_running() {
        start_daemon()?;
    }
    let current = status();
    if !current.ready() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "Computer Use is not ready: {}. Run appforge computer setup.",
                current.summary()
            ),
        ));
    }

    let controller = controller(cfg).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "No enabled Codex/Claude controller is available for Computer Use",
        )
    })?;
    let binary = provider::find_binary(&controller)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "controller CLI missing"))?;
    let cwd = cwd.canonicalize()?;
    let full_prompt = format!(
        "{}\n\nTASK:\n{}",
        r#"COMPUTER-USE CONTRACT:
- This is a bounded smoke-test task. Use only this project and the computer MCP.
- Do not inspect credentials, unrelated tabs, global skills, or account settings.
- Do not publish, upload, purchase, submit, or accept agreements.
- Do not modify application source code. Record observations only.
- Stop quickly if a simulator/browser target cannot be launched with existing project tooling.
- Write the requested durable result file before exiting."#,
        prompt
    );

    let _ = tx.send(format!(
        "{} + CUA Driver ▶ bounded UI smoke test",
        provider::label(&controller)
    ));
    let command = match controller.as_str() {
        "codex" => codex_command(&binary, &cwd, &full_prompt, false),
        "claude" => claude_command(&binary, &cwd, &full_prompt, false),
        _ => unreachable!(),
    };
    stream_command(command, provider::label(&controller), tx, Some(timeout))?;
    Ok(controller)
}

pub fn policy_and_store_prompt(cfg: &Config, project: &Path, brief: &str) -> String {
    let notion = if cfg.notion_enabled {
        format!(
            r#"
NOTION POLICY PUBLICATION:
- Target type: {kind}
- Exact target URL: {url}
- Read the generated policy files under docs/policies/.
- If target type is page, create a dedicated child page for this app's policies. Never publish the configured parent page itself.
- If target type is database, create a NEW app-specific row/page. Never publish the database, an existing row, or change database sharing.
- Create at minimum Privacy Policy, Terms/Usage Terms when applicable, and Support/Data Deletion information.
- Public publishing requested: {public}.
- If public publishing is requested, publish only the newly created policy page(s) containing policy text alone; exclude child pages, linked databases, embeds, internal notes, and source files, verify the public URL in a logged-out/publicly accessible view when possible, and remember that publishing a Notion parent can expose subpages.
- Save the resulting public URLs and Notion workspace URLs to .appforge/policy-links.conf.
- Replace PUBLIC_PRIVACY_POLICY_URL, PUBLIC_TERMS_URL, and PUBLIC_SUPPORT_URL placeholders in docs/06-store.md with verified public URLs when available.
- If public publishing is disabled or a public URL cannot be verified, leave the placeholder unresolved and record a blocker; never substitute a private workspace URL as a public policy URL.
"#,
            kind = cfg.notion_target_kind,
            url = cfg.notion_target_url,
            public = cfg.notion_publish_public,
        )
    } else {
        "NOTION POLICY PUBLICATION: disabled. Do not change Notion.".into()
    };

    let store: String = if cfg.store_draft_upload {
        r#"
STORE DRAFT UPLOAD:
- Read docs/06-store.md, docs/05-qa.md, docs/04-quality.md, .appforge/policy-links.conf, and existing store/build metadata.
- Open the exact existing app records in Google Play Console and/or App Store Connect by package/bundle identifier. If an exact matching app record cannot be proven, stop and record a blocker rather than creating a new app record.
- Fill draft listing metadata and privacy-policy URLs from the generated documents.
- Upload screenshots and existing signed build artifacts only when their exact project/app identity is verified.
- Read .appforge/publish-approved.conf. It is a one-attempt human approval generated by AppForge and must exactly match the current Notion target and store-upload request. Never broaden its actions.
- Save changes that remain editable drafts. Never replace/delete existing releases, builds, screenshots, or listing content. Verify approved identifiers and artifact SHA-256 against actual files before any store mutation; missing/mismatched approval is a blocker.
- Do NOT submit for review, roll out to production, publish a release, accept agreements, change pricing, or create irreversible store identifiers.
"#
        .into()
    } else {
        "STORE DRAFT UPLOAD: disabled. Do not change Play Console or App Store Connect.".into()
    };

    format!(
        r#"You are the release computer-use operator for the generated mobile app.

Project: {}
Product brief: {}

{}
{}

Write docs/07-publish.md with:
- controller and computer-use backend used
- Notion pages/public URLs created or verified
- Google Play/App Store draft fields and artifacts uploaded
- exact blockers requiring human action
- explicit confirmation that no final review/production submission was performed
"#,
        project.display(),
        brief,
        notion,
        store
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readiness_fails_closed() {
        assert!(!daemon_ready(true, "Cua Driver daemon is not running"));
        assert!(daemon_ready(true, "Cua Driver daemon is running"));
        assert!(!permissions_ready(
            false,
            "Accessibility: ✅\nScreen Recording: ✅"
        ));
        assert!(!permissions_ready(
            true,
            "Accessibility: ✅✅\nScreen Recording: unknown"
        ));
    }

    #[test]
    fn codex_command_injects_cua_without_dangerous_bypass() {
        let cmd = codex_command(Path::new("codex"), Path::new("/tmp"), "brief", true);
        let args = cmd
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(args
            .iter()
            .any(|arg| arg.contains("mcp_servers.computer.command")));
        assert!(args.iter().any(|arg| arg == "gpt-5.6-sol"));
        assert!(args.iter().any(|arg| arg == "--skip-git-repo-check"));
        assert!(args.iter().any(|arg| arg == "skip_host_skill_discovery"));
        assert!(args.iter().any(|arg| arg == "--approve-for-me"));
        assert!(!args.iter().any(|arg| arg == "--sandbox"));
        assert!(!args.iter().any(|arg| arg == "approval_policy=\"never\""));
        assert!(!args
            .iter()
            .any(|arg| arg == "--dangerously-bypass-approvals-and-sandbox"));
    }

    #[test]
    fn policy_prompt_does_not_allow_final_store_submission() {
        let cfg = Config {
            notion_enabled: true,
            notion_target_url: "https://notion.so/example".into(),
            store_draft_upload: true,
            ..Config::default()
        };
        let prompt = policy_and_store_prompt(&cfg, Path::new("/tmp/app"), "game");
        assert!(prompt.contains("Do NOT submit for review"));
        assert!(prompt.contains("Never publish the configured parent page itself"));
    }
}
