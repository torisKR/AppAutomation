use std::env;
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::Sender;
use std::thread;

#[derive(Clone, Debug)]
pub struct ProviderInfo {
    pub id: &'static str,
    pub label: &'static str,
    pub binary: Option<PathBuf>,
    pub installed: bool,
    pub auth_scheme: &'static str,
    pub auth_summary: String,
}

#[derive(Clone, Debug)]
pub enum AuthState {
    Authenticated(String),
    NeedsLogin(String),
    Unsupported(String),
    Missing,
}

pub fn ids() -> &'static [&'static str] {
    &["codex", "claude", "cursor", "antigravity", "opencode"]
}

pub fn label(id: &str) -> &'static str {
    match id {
        "codex" => "Codex",
        "claude" => "Claude Code",
        "cursor" => "Cursor",
        "antigravity" => "Antigravity",
        "opencode" => "OpenCode Go",
        _ => "Unknown",
    }
}

fn auth_scheme(id: &str) -> &'static str {
    match id {
        "codex" => "OAuth/ChatGPT",
        "claude" => "OAuth/Claude",
        "cursor" => "OAuth/Cursor",
        "antigravity" => "OAuth/Google",
        "opencode" => "Subscription key",
        _ => "unknown",
    }
}

fn candidates(id: &str) -> &'static [&'static str] {
    match id {
        "codex" => &["codex"],
        "claude" => &["claude"],
        "cursor" => &["agent", "cursor-agent"],
        "antigravity" => &["agy"],
        "opencode" => &["opencode"],
        _ => &[],
    }
}

fn find_binary(id: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    for dir in env::split_paths(&path) {
        for name in candidates(id) {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

pub fn detected() -> Vec<ProviderInfo> {
    ids()
        .iter()
        .map(|id| {
            let binary = find_binary(id);
            let installed = binary.is_some();
            let auth_summary = match auth_status(id) {
                AuthState::Authenticated(v) => format!("ready · {v}"),
                AuthState::NeedsLogin(v) => format!("login needed · {v}"),
                AuthState::Unsupported(v) => v,
                AuthState::Missing => "not installed".into(),
            };
            ProviderInfo {
                id,
                label: label(id),
                binary,
                installed,
                auth_scheme: auth_scheme(id),
                auth_summary,
            }
        })
        .collect()
}

fn output_status(binary: &Path, args: &[&str]) -> io::Result<(bool, String)> {
    let mut command = Command::new(binary);
    // Status checks use the same subscription-first environment as task runs.
    let id = ids()
        .iter()
        .find(|id| {
            candidates(id)
                .iter()
                .any(|name| binary.file_name().is_some_and(|file| file == *name))
        })
        .copied()
        .unwrap_or("");
    subscription_environment(&mut command, id);
    let output = command.args(args).stdin(Stdio::null()).output()?;
    let mut text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !stderr.is_empty() {
        if !text.is_empty() {
            text.push_str(" · ");
        }
        text.push_str(&stderr);
    }
    Ok((output.status.success(), compact(&text, 120)))
}

pub fn auth_status(id: &str) -> AuthState {
    let Some(binary) = find_binary(id) else {
        return AuthState::Missing;
    };
    match id {
        "codex" => match output_status(&binary, &["login", "status"]) {
            Ok((true, text)) => AuthState::Authenticated(nonempty(text, "ChatGPT session")),
            Ok((false, text)) => AuthState::NeedsLogin(nonempty(text, "run codex login")),
            Err(err) => AuthState::NeedsLogin(err.to_string()),
        },
        "claude" => match output_status(&binary, &["auth", "status", "--text"]) {
            Ok((true, text)) => AuthState::Authenticated(nonempty(text, "Claude session")),
            Ok((false, text)) => AuthState::NeedsLogin(nonempty(text, "run claude auth login")),
            Err(err) => AuthState::NeedsLogin(err.to_string()),
        },
        "cursor" => match output_status(&binary, &["status"]) {
            Ok((true, text)) => AuthState::Authenticated(nonempty(text, "Cursor session")),
            Ok((false, text)) => AuthState::NeedsLogin(nonempty(text, "run agent login")),
            Err(err) => AuthState::NeedsLogin(err.to_string()),
        },
        "antigravity" => AuthState::Unsupported(
            "installed; agy caches Google OAuth after first interactive launch".into(),
        ),
        // A successful list command (even with a banner) does not prove that
        // credentials exist, or that the selected model uses a Go subscription.
        "opencode" => AuthState::Unsupported(
            "verify subscription provider with opencode auth list; strict auth blocks tasks".into(),
        ),
        _ => AuthState::Missing,
    }
}

pub fn login(id: &str) -> io::Result<()> {
    let binary = find_binary(id)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("{id} CLI missing")))?;
    let args: &[&str] = match id {
        "codex" => &["login"],
        "claude" => &["auth", "login"],
        "cursor" => &["login"],
        "antigravity" => &[],
        "opencode" => &["auth", "login"],
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "unknown provider",
            ))
        }
    };
    let mut command = Command::new(binary);
    subscription_environment(&mut command, id);
    let status = command
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!("{id} login exited with {status}")))
    }
}

fn nonempty(value: String, fallback: &str) -> String {
    if value.trim().is_empty() {
        fallback.into()
    } else {
        value
    }
}

fn compact(value: &str, max: usize) -> String {
    let one_line = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() <= max {
        return one_line;
    }
    one_line
        .chars()
        .take(max.saturating_sub(1))
        .collect::<String>()
        + "…"
}

fn subscription_environment(cmd: &mut Command, id: &str) {
    let removed: &[&str] = match id {
        "codex" => &["OPENAI_API_KEY", "CODEX_API_KEY", "CODEX_ACCESS_TOKEN"],
        "claude" => &[
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_AUTH_TOKEN",
            "CLAUDE_CODE_OAUTH_TOKEN",
            "ANTHROPIC_AWS_API_KEY",
            "ANTHROPIC_FOUNDRY_API_KEY",
            "CLAUDE_CODE_USE_BEDROCK",
            "CLAUDE_CODE_USE_VERTEX",
            "CLAUDE_CODE_USE_FOUNDRY",
        ],
        "cursor" => &["CURSOR_API_KEY"],
        "antigravity" => &["GEMINI_API_KEY", "GOOGLE_API_KEY"],
        _ => &[],
    };
    for name in removed {
        cmd.env_remove(name);
    }
}

fn configure_command(
    id: &str,
    binary: &Path,
    cwd: &Path,
    prompt: &str,
    auto: bool,
) -> io::Result<Command> {
    let mut cmd = Command::new(binary);
    cmd.current_dir(cwd);
    // Workers must not consume keyboard input or terminal mouse reports.
    cmd.stdin(Stdio::null());
    subscription_environment(&mut cmd, id);
    match id {
        "codex" => {
            cmd.args([
                "exec",
                "--json",
                "--sandbox",
                "workspace-write",
                "-c",
                "forced_login_method=\"chatgpt\"",
                "-c",
                "model_provider=\"openai\"",
            ]);
            if auto {
                cmd.arg("--approve-for-me");
            } else {
                // Headless manual stages deny requests requiring approval.
                cmd.args(["-c", "approval_policy=\"never\""]);
            }
            cmd.arg("-C").arg(cwd).arg("--").arg(prompt);
        }
        "claude" => {
            cmd.args([
                "-p",
                "--verbose",
                "--output-format",
                "stream-json",
                "--permission-mode",
                if auto { "auto" } else { "acceptEdits" },
                "--permission-prompts",
                "none",
                "--",
            ])
            .arg(prompt);
        }
        "cursor" => {
            // Auto scheduling is not permission bypass. Retain sandbox and
            // the safe-call classifier in both scheduling modes.
            cmd.args([
                "-p",
                "--output-format",
                "stream-json",
                "--sandbox",
                "enabled",
                "--trust",
                "--auto-review",
                "--workspace",
            ])
            .arg(cwd)
            .arg("--")
            .arg(prompt);
        }
        "antigravity" => {
            cmd.args([
                "-p",
                prompt,
                "--output-format",
                "stream-json",
                "--sandbox",
                "--print-timeout",
                "30m",
            ]);
        }
        "opencode" => {
            // Never translate AUTO into the dangerous --auto permission flag.
            cmd.args(["run", "--format", "json", "--dir"])
                .arg(cwd)
                .arg("--")
                .arg(prompt);
        }
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "unknown provider",
            ))
        }
    }
    Ok(cmd)
}

pub fn run_task(
    id: &str,
    cwd: &Path,
    prompt: &str,
    auto: bool,
    strict_subscription_auth: bool,
    tx: Sender<String>,
) -> io::Result<()> {
    if strict_subscription_auth && id == "opencode" {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "OpenCode Go uses a subscription API key, not OAuth. Disable strict_subscription_auth to allow it.",
        ));
    }

    let binary = find_binary(id)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("{id} CLI missing")))?;
    let cwd = cwd.canonicalize()?;
    let _ = tx.send(format!("{} ▶ {}", label(id), compact(prompt, 120)));
    let mut child = configure_command(id, &binary, &cwd, prompt, auto)?
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let tx_out = tx.clone();
    let out_handle = thread::spawn(move || {
        if let Some(out) = stdout {
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                let _ = tx_out.send(format!("│ {line}"));
            }
        }
    });

    let tx_err = tx.clone();
    let err_handle = thread::spawn(move || {
        if let Some(err) = stderr {
            for line in BufReader::new(err).lines().map_while(Result::ok) {
                let _ = tx_err.send(format!("! {line}"));
            }
        }
    });

    let status = child.wait()?;
    let _ = out_handle.join();
    let _ = err_handle.join();

    if status.success() {
        let _ = tx.send(format!("{} ✓ completed", label(id)));
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "{} exited with {}",
            label(id),
            status
        )))
    }
}

pub fn doctor_line(info: &ProviderInfo) -> String {
    let binary = info
        .binary
        .as_ref()
        .map(|v| v.display().to_string())
        .unwrap_or_else(|| "-".into());
    format!(
        "{:<12} {:<5} {:<18} {:<36} {}",
        info.label,
        if info.installed { "yes" } else { "no" },
        info.auth_scheme,
        compact(&info.auth_summary, 36),
        binary
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_are_stable() {
        assert_eq!(label("codex"), "Codex");
        assert_eq!(label("antigravity"), "Antigravity");
    }

    #[test]
    fn compact_is_bounded() {
        let s = compact("abcdefghijklmnopqrstuvwxyz", 8);
        assert!(s.chars().count() <= 8);
    }
    fn args(id: &str, auto: bool) -> Vec<String> {
        configure_command(
            id,
            Path::new("provider"),
            Path::new("/tmp/project"),
            "--brief",
            auto,
        )
        .unwrap()
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect()
    }

    #[test]
    fn scheduling_never_enables_permission_bypass() {
        for id in ids() {
            for auto in [false, true] {
                let arguments = args(id, auto);
                for forbidden in [
                    "--force",
                    "--yolo",
                    "--dangerously-skip-permissions",
                    "--dangerously-bypass-approvals-and-sandbox",
                    "--auto",
                ] {
                    assert!(
                        !arguments.iter().any(|arg| arg == forbidden),
                        "{id}: {arguments:?}"
                    );
                }
            }
        }
        assert!(args("codex", true).contains(&"--approve-for-me".into()));
        assert!(args("codex", false).contains(&"approval_policy=\"never\"".into()));
        assert!(args("codex", true).contains(&"forced_login_method=\"chatgpt\"".into()));
        assert!(args("claude", true).contains(&"--verbose".into()));
    }

    #[test]
    fn prompt_cannot_be_parsed_as_a_flag() {
        for id in ["codex", "claude", "cursor", "opencode"] {
            let arguments = args(id, true);
            assert_eq!(arguments[arguments.len() - 2..], ["--", "--brief"]);
        }
        let arguments = args("antigravity", true);
        assert_eq!(&arguments[..2], ["-p", "--brief"]);
    }

    #[test]
    fn subscription_keys_are_removed_without_reading_them() {
        for (id, key) in [
            ("codex", "OPENAI_API_KEY"),
            ("codex", "CODEX_ACCESS_TOKEN"),
            ("claude", "ANTHROPIC_API_KEY"),
            ("claude", "ANTHROPIC_AUTH_TOKEN"),
            ("claude", "CLAUDE_CODE_USE_BEDROCK"),
            ("cursor", "CURSOR_API_KEY"),
            ("antigravity", "GEMINI_API_KEY"),
            ("antigravity", "GOOGLE_API_KEY"),
        ] {
            let command =
                configure_command(id, Path::new("provider"), Path::new("/tmp"), "brief", true)
                    .unwrap();
            assert!(command
                .get_envs()
                .any(|(name, value)| name == key && value.is_none()));
        }
    }

    #[test]
    fn strict_auth_blocks_opencode_before_starting_a_cli() {
        let (tx, _) = std::sync::mpsc::channel();
        let err = run_task("opencode", Path::new("/tmp"), "brief", true, true, tx).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
        assert!(configure_command(
            "unknown",
            Path::new("provider"),
            Path::new("/tmp"),
            "brief",
            true
        )
        .is_err());
    }
}
