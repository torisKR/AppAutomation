use std::env;
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;

use crate::computer;
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
    pub notion_enabled: bool,
    pub notion_target_kind: String,
    pub notion_target_url: String,
    pub notion_publish_public: bool,
    pub computer_backend: String,
    pub store_draft_upload: bool,
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
            notion_enabled: false,
            notion_target_kind: "page".into(),
            notion_target_url: String::new(),
            notion_publish_public: false,
            computer_backend: "auto".into(),
            store_draft_upload: false,
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
            "primary={}\nsecondary={}\nenabled={}\nprojects_dir={}\nauto_mode={}\naside_enabled={}\nstrict_subscription_auth={}\nnotion_enabled={}\nnotion_target_kind={}\nnotion_target_url={}\nnotion_publish_public={}\ncomputer_backend={}\nstore_draft_upload={}\n",
            self.primary,
            self.secondary.join(","),
            self.enabled.join(","),
            self.projects_dir.display(),
            self.auto_mode,
            self.aside_enabled,
            self.strict_subscription_auth,
            self.notion_enabled,
            self.notion_target_kind,
            self.notion_target_url,
            self.notion_publish_public,
            self.computer_backend,
            self.store_draft_upload,
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
            "notion_enabled" => cfg.notion_enabled = parse_bool(value, cfg.notion_enabled),
            "notion_target_kind" => cfg.notion_target_kind = value.to_string(),
            "notion_target_url" => cfg.notion_target_url = value.to_string(),
            "notion_publish_public" => {
                cfg.notion_publish_public = parse_bool(value, cfg.notion_publish_public)
            }
            "computer_backend" => cfg.computer_backend = value.to_string(),
            "store_draft_upload" => {
                cfg.store_draft_upload = parse_bool(value, cfg.store_draft_upload)
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
    if io::stdin().read_line(&mut value)? == 0 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "setup input closed",
        ));
    }
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

fn yes_explicit(raw: &str) -> bool {
    matches!(raw.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

fn yes_default(raw: &str) -> bool {
    !matches!(raw.to_ascii_lowercase().as_str(), "n" | "no")
}

pub fn onboard() -> io::Result<Config> {
    if !io::stdin().is_terminal() {
        return Err(io::Error::other("setup requires an interactive terminal"));
    }
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

    let auto_mode = yes_default(&read_line("기본 자동 실행? [Y/n]: ")?);
    let aside_enabled = yes_default(&read_line("Aside Browser lane 사용? [Y/n]: ")?);

    println!("\n정책 문서 자동 배포");
    println!("개인정보처리방침/이용약관/지원 문서를 Notion에 만들고 스토어 메타데이터에 공개 URL을 연결할 수 있습니다.");
    let notion_enabled = yes_explicit(&read_line(
        "Notion 정책 문서 자동 배포를 사용할까요? [y/N]: ",
    )?);
    let mut notion_target_kind = "page".to_string();
    let mut notion_target_url = String::new();
    let mut notion_publish_public = false;
    if notion_enabled {
        println!("  1. Notion 페이지 아래에 정책용 하위 페이지 생성");
        println!("  2. Notion 데이터베이스에 앱별 정책 페이지 생성");
        let kind = choose_index("저장 대상 [1]: ", 2, 0)?;
        notion_target_kind = if kind == 1 { "database" } else { "page" }.into();
        notion_target_url = read_line("Notion 대상 페이지/데이터베이스 URL: ")?;
        if notion_target_url.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Notion 자동 배포를 사용하려면 대상 URL이 필요합니다.",
            ));
        }
        notion_publish_public = yes_explicit(&read_line(
            "notion.site 공개 링크까지 자동 게시할까요? [y/N]: ",
        )?);
    }

    println!("\nComputer Use controller");
    println!("  1. Auto (Primary가 Codex/Claude면 해당 구독 세션 + CUA Driver)");
    println!("  2. Codex + CUA Driver");
    println!("  3. Claude + CUA Driver");
    let requested_backend = match choose_index("백엔드 [1]: ", 3, 0)? {
        1 => "codex",
        2 => "claude",
        _ => "auto",
    };
    let computer_backend =
        if requested_backend != "auto" && !enabled.iter().any(|id| id == requested_backend) {
            println!(
            "  선택한 {} provider가 활성화되어 있지 않아 Computer Use 백엔드는 Auto로 저장합니다.",
            requested_backend
        );
            "auto".to_string()
        } else {
            requested_backend.to_string()
        };

    let store_draft_upload = yes_explicit(&read_line(
        "Play Console / App Store Connect에 메타데이터·빌드를 Draft 상태까지 자동 업로드할까요? [y/N]: ",
    )?);
    if store_draft_upload {
        println!("  최종 심사 제출/프로덕션 출시/약관 동의는 자동으로 누르지 않습니다.");
    }

    if (notion_enabled || store_draft_upload)
        && !enabled
            .iter()
            .any(|id| matches!(id.as_str(), "codex" | "claude"))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Computer Use 자동화를 사용하려면 Codex 또는 Claude를 enabled provider에 포함해야 합니다.",
        ));
    }

    let mut cfg = Config {
        primary,
        secondary,
        enabled,
        projects_dir,
        auto_mode,
        aside_enabled,
        strict_subscription_auth: true,
        notion_enabled,
        notion_target_kind,
        notion_target_url,
        notion_publish_public,
        computer_backend,
        store_draft_upload,
    };

    if cfg.enabled.iter().any(|id| id == "opencode") {
        println!(
            "\n주의: OpenCode Go는 구독 상품이지만 공식 연결 방식은 API key입니다. OAuth-only 정책에서는 자동 실행 대상에서 제외됩니다."
        );
        cfg.strict_subscription_auth = true;
    }

    if cfg.notion_enabled || cfg.store_draft_upload {
        let status = computer::status();
        println!("\nComputer Use: {}", status.summary());
        if !status.cua_installed {
            let install = yes_default(&read_line(
                "CUA Driver가 없습니다. 공식 설치 스크립트로 설치할까요? [Y/n]: ",
            )?);
            if install {
                computer::install_cua()?;
            }
        }
        let status = computer::status();
        if status.cua_installed && !status.permissions_ready {
            let grant = yes_default(&read_line(
                "CUA Driver의 Accessibility/Screen Recording 권한을 설정할까요? [Y/n]: ",
            )?);
            if grant {
                computer::grant_permissions()?;
            }
        }
    }

    fs::create_dir_all(&cfg.projects_dir)?;
    cfg.save()?;
    println!("\n설정 저장: {}", config_path().display());

    let login_now =
        read_line("선택한 provider 로그인 상태를 확인하고 필요한 로그인을 실행할까요? [Y/n]: ")?;
    if yes_default(&login_now) {
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
                    if yes_default(&answer) {
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
    fn legacy_config_keeps_external_actions_disabled() {
        let cfg = parse("primary=claude\nsecondary=codex\nenabled=claude,codex\n");
        assert_eq!(cfg.primary, "claude");
        assert!(!cfg.notion_enabled && !cfg.store_draft_upload && !cfg.notion_publish_public);
        assert!(cfg.strict_subscription_auth);
        assert!(!yes_explicit(""));
    }

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
            notion_enabled: true,
            notion_target_kind: "database".into(),
            notion_target_url: "https://notion.so/example".into(),
            notion_publish_public: true,
            computer_backend: "codex".into(),
            store_draft_upload: true,
        };
        let parsed = parse(&cfg.serialize());
        assert_eq!(parsed.primary, "claude");
        assert_eq!(parsed.secondary, vec!["codex", "cursor"]);
        assert_eq!(parsed.projects_dir, PathBuf::from("/tmp/games"));
        assert!(!parsed.auto_mode);
        assert!(parsed.aside_enabled);
        assert!(parsed.notion_enabled);
        assert_eq!(parsed.notion_target_kind, "database");
        assert_eq!(parsed.notion_target_url, "https://notion.so/example");
        assert_eq!(parsed.computer_backend, "codex");
        assert!(parsed.store_draft_upload);
    }
}
