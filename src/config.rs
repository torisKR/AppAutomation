use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;

use crate::provider;

#[derive(Clone, Debug)]
pub struct Config {
    pub primary: String,
    pub secondary: Vec<String>,
    pub enabled: Vec<String>,
    pub projects_dir: PathBuf,
    pub auto_mode: bool,
    pub aside_enabled: bool,
    pub strict_subscription_auth: bool,
}

impl Default for Config {
    fn default() -> Self {
        let home = env::var("HOME").unwrap_or_else(|_| ".".into());
        Self {
            primary: "codex".into(),
            secondary: vec!["claude".into()],
            enabled: vec!["codex".into(), "claude".into()],
            projects_dir: PathBuf::from(home).join("projects/appforge-games"),
            auto_mode: true,
            aside_enabled: true,
            strict_subscription_auth: true,
        }
    }
}

pub fn config_path() -> PathBuf {
    if let Ok(base) = env::var("XDG_CONFIG_HOME") {
        return PathBuf::from(base).join("appforge/config");
    }
    let home = env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".config/appforge/config")
}

impl Config {
    pub fn load() -> io::Result<Self> {
        let path = config_path();
        let text = fs::read_to_string(path)?;
        Ok(parse(&text))
    }

    pub fn save(&self) -> io::Result<()> {
        let path = config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, self.serialize())
    }

    pub fn serialize(&self) -> String {
        format!(
            "primary={}\nsecondary={}\nenabled={}\nprojects_dir={}\nauto_mode={}\naside_enabled={}\nstrict_subscription_auth={}\n",
            self.primary,
            self.secondary.join(","),
            self.enabled.join(","),
            self.projects_dir.display(),
            self.auto_mode,
            self.aside_enabled,
            self.strict_subscription_auth,
        )
    }
}

fn parse(text: &str) -> Config {
    let mut cfg = Config::default();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "primary" => cfg.primary = value.to_string(),
            "secondary" => cfg.secondary = csv(value),
            "enabled" => cfg.enabled = csv(value),
            "projects_dir" => cfg.projects_dir = expand_home(value),
            "auto_mode" => cfg.auto_mode = parse_bool(value, cfg.auto_mode),
            "aside_enabled" => cfg.aside_enabled = parse_bool(value, cfg.aside_enabled),
            "strict_subscription_auth" => {
                cfg.strict_subscription_auth = parse_bool(value, cfg.strict_subscription_auth)
            }
            _ => {}
        }
    }
    cfg
}

fn csv(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
        .collect()
}

fn parse_bool(value: &str, fallback: bool) -> bool {
    match value.to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "y" => true,
        "false" | "0" | "no" | "n" => false,
        _ => fallback,
    }
}

fn expand_home(value: &str) -> PathBuf {
    if let Some(rest) = value.strip_prefix("~/") {
        if let Ok(home) = env::var("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(value)
}

fn read_line(prompt: &str) -> io::Result<String> {
    print!("{prompt}");
    io::stdout().flush()?;
    let mut value = String::new();
    io::stdin().read_line(&mut value)?;
    Ok(value.trim().to_string())
}

fn choose_index(prompt: &str, max: usize, default: usize) -> io::Result<usize> {
    loop {
        let raw = read_line(prompt)?;
        if raw.is_empty() {
            return Ok(default.min(max.saturating_sub(1)));
        }
        if let Ok(n) = raw.parse::<usize>() {
            if n >= 1 && n <= max {
                return Ok(n - 1);
            }
        }
        println!("  1..={max} 범위의 번호를 입력하세요.");
    }
}

fn selected_from_indices(raw: &str, installed: &[String]) -> Vec<String> {
    if raw.trim().is_empty() {
        return installed.to_vec();
    }
    let mut out = Vec::new();
    for part in raw.split(',') {
        let idx = part.trim().parse::<usize>().ok();
        if let Some(i) = idx.and_then(|n| n.checked_sub(1)) {
            if let Some(id) = installed.get(i) {
                if !out.contains(id) {
                    out.push(id.clone());
                }
            }
        }
    }
    out
}

pub fn onboard() -> io::Result<Config> {
    println!("\nAppForge first-run setup");
    println!("========================");
    println!("각 AI CLI는 토큰을 추출하지 않고 공식 CLI 로그인 세션을 그대로 사용합니다.\n");

    let detected = provider::detected();
    let installed: Vec<String> = detected
        .iter()
        .filter(|p| p.installed)
        .map(|p| p.id.to_string())
        .collect();

    if installed.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "지원되는 AI CLI가 하나도 설치되어 있지 않습니다.",
        ));
    }

    println!("설치된 provider:");
    for (i, id) in installed.iter().enumerate() {
        let info = detected.iter().find(|p| p.id == id).unwrap();
        println!(
            "  {}. {:12} {:14} {}",
            i + 1,
            info.label,
            info.auth_scheme,
            info.auth_summary
        );
    }

    let raw = read_line("\n사용할 AI 번호를 쉼표로 선택 [Enter=모두]: ")?;
    let mut enabled = selected_from_indices(&raw, &installed);
    if enabled.is_empty() {
        enabled = installed.clone();
    }

    println!("\nPrimary orchestrator:");
    for (i, id) in enabled.iter().enumerate() {
        println!("  {}. {}", i + 1, provider::label(id));
    }
    let primary_idx = choose_index("Primary 번호 [1]: ", enabled.len(), 0)?;
    let primary = enabled[primary_idx].clone();
    let secondary = enabled
        .iter()
        .filter(|id| **id != primary)
        .cloned()
        .collect::<Vec<_>>();

    let default_dir = Config::default().projects_dir;
    let dir_raw = read_line(&format!("생성 앱 루트 [{}]: ", default_dir.display()))?;
    let projects_dir = if dir_raw.is_empty() {
        default_dir
    } else {
        expand_home(&dir_raw)
    };

    let auto_raw = read_line("기본 자동 실행? [Y/n]: ")?;
    let auto_mode = !matches!(auto_raw.to_ascii_lowercase().as_str(), "n" | "no");

    let aside_raw = read_line("Aside Browser lane 사용? [Y/n]: ")?;
    let aside_enabled = !matches!(aside_raw.to_ascii_lowercase().as_str(), "n" | "no");

    let mut cfg = Config {
        primary,
        secondary,
        enabled,
        projects_dir,
        auto_mode,
        aside_enabled,
        strict_subscription_auth: true,
    };

    if cfg.enabled.iter().any(|id| id == "opencode") {
        println!(
            "\n주의: OpenCode Go는 구독 상품이지만 공식 연결 방식은 API key입니다. OAuth-only 정책에서는 자동 실행 대상에서 제외됩니다."
        );
        cfg.strict_subscription_auth = true;
    }

    fs::create_dir_all(&cfg.projects_dir)?;
    cfg.save()?;
    println!("\n설정 저장: {}", config_path().display());

    let login_now =
        read_line("선택한 provider 로그인 상태를 확인하고 필요한 로그인을 실행할까요? [Y/n]: ")?;
    if !matches!(login_now.to_ascii_lowercase().as_str(), "n" | "no") {
        for id in &cfg.enabled {
            match provider::auth_status(id) {
                provider::AuthState::Authenticated(note) => {
                    println!("  ✓ {}: {}", provider::label(id), note)
                }
                provider::AuthState::Unsupported(note) => {
                    println!("  ! {}: {}", provider::label(id), note)
                }
                provider::AuthState::Missing => {}
                provider::AuthState::NeedsLogin(note) => {
                    println!("\n{} 로그인 필요: {}", provider::label(id), note);
                    let answer = read_line("  지금 로그인 실행? [Y/n]: ")?;
                    if !matches!(answer.to_ascii_lowercase().as_str(), "n" | "no") {
                        let _ = provider::login(id);
                    }
                }
            }
        }
    }

    Ok(cfg)
}

pub fn ensure() -> io::Result<Config> {
    match Config::load() {
        Ok(cfg) => Ok(cfg),
        Err(err) if err.kind() == io::ErrorKind::NotFound => onboard(),
        Err(err) => Err(err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_round_trip_core_fields() {
        let cfg = Config {
            primary: "claude".into(),
            secondary: vec!["codex".into(), "cursor".into()],
            enabled: vec!["claude".into(), "codex".into(), "cursor".into()],
            projects_dir: PathBuf::from("/tmp/games"),
            auto_mode: false,
            aside_enabled: true,
            strict_subscription_auth: true,
        };
        let parsed = parse(&cfg.serialize());
        assert_eq!(parsed.primary, "claude");
        assert_eq!(parsed.secondary, vec!["codex", "cursor"]);
        assert_eq!(parsed.projects_dir, PathBuf::from("/tmp/games"));
        assert!(!parsed.auto_mode);
        assert!(parsed.aside_enabled);
    }
}
