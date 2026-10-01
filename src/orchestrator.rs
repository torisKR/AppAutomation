use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

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

fn stage_timeout(stage: Stage) -> std::time::Duration {
    let minutes = match stage {
        Stage::Plan | Stage::Design | Stage::Store | Stage::Release => 10,
        Stage::Build => 25,
        Stage::Quality => 20,
        Stage::Qa => 15,
        Stage::Publish => 15,
    };
    std::time::Duration::from_secs(minutes * 60)
}

fn run_host_command_bounded(
    mut command: Command,
    log_path: &Path,
    timeout: Duration,
) -> io::Result<()> {
    if let Some(parent) = log_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let stdout = fs::File::create(log_path)?;
    let stderr = stdout.try_clone()?;
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()?;

    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return if status.success() {
                Ok(())
            } else {
                Err(io::Error::other(format!(
                    "host verification command exited with {status}; see {}",
                    log_path.display()
                )))
            };
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!(
                    "host verification command timed out after {}s; see {}",
                    timeout.as_secs(),
                    log_path.display()
                ),
            ));
        }
        thread::sleep(Duration::from_millis(500));
    }
}

#[cfg(target_os = "macos")]
fn java_17_home() -> Option<String> {
    let output = Command::new("/usr/libexec/java_home")
        .args(["-v", "17"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!value.is_empty()).then_some(value)
}

#[cfg(not(target_os = "macos"))]
fn java_17_home() -> Option<String> {
    std::env::var("JAVA_HOME")
        .ok()
        .filter(|value| !value.is_empty())
}

fn android_sdk_home() -> Option<PathBuf> {
    for name in ["ANDROID_HOME", "ANDROID_SDK_ROOT"] {
        if let Some(path) = std::env::var_os(name).map(PathBuf::from) {
            if path.is_dir() {
                return Some(path);
            }
        }
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    let home = std::env::var_os("HOME").map(PathBuf::from);
    #[cfg(target_os = "macos")]
    if let Some(path) = home.as_ref().map(|home| home.join("Library/Android/sdk")) {
        if path.is_dir() {
            return Some(path);
        }
    }

    #[cfg(target_os = "linux")]
    if let Some(path) = home.as_ref().map(|home| home.join("Android/Sdk")) {
        if path.is_dir() {
            return Some(path);
        }
    }

    #[cfg(windows)]
    if let Some(path) = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .map(|base| base.join("Android/Sdk"))
    {
        if path.is_dir() {
            return Some(path);
        }
    }

    None
}

fn expo_project(project: &Path) -> bool {
    fs::read_to_string(project.join("package.json"))
        .map(|text| text.contains("\"expo\""))
        .unwrap_or(false)
}

fn ensure_expo_native_dir(
    project: &Path,
    platform: &str,
    marker: &Path,
    tx: &Sender<String>,
) -> io::Result<()> {
    if marker.exists() {
        return Ok(());
    }
    let expo = project.join("node_modules/.bin/expo");
    if !expo.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "Expo native project is missing and node_modules/.bin/expo is unavailable",
        ));
    }
    let _ = tx.send(format!(
        "Host native verify ▶ generating {platform} native project"
    ));
    let mut command = Command::new(expo);
    command
        .current_dir(project)
        .args(["prebuild", "--platform", platform, "--no-install"])
        .env("NODE_ENV", "production");
    run_host_command_bounded(
        command,
        &project.join(format!(".appforge/host-prebuild-{platform}.log")),
        Duration::from_secs(300),
    )
}

fn verify_android_host(project: &Path, tx: &Sender<String>) -> io::Result<()> {
    let android = project.join("android");
    ensure_expo_native_dir(project, "android", &android, tx)?;

    #[cfg(windows)]
    let wrapper = android.join("gradlew.bat");
    #[cfg(not(windows))]
    let wrapper = android.join("gradlew");

    if !wrapper.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "Android Gradle wrapper is missing after prebuild",
        ));
    }

    let _ = tx.send("Host native verify ▶ Android release assemble".into());
    let gradle_home = project.join(".appforge/gradle-home");
    let android_user_home = project.join(".appforge/android-home");
    let tmp = project.join(".appforge/tmp");
    fs::create_dir_all(&gradle_home)?;
    fs::create_dir_all(&android_user_home)?;
    fs::create_dir_all(&tmp)?;

    let mut command = Command::new(wrapper);
    command
        .current_dir(project)
        .args(["-p", "android", ":app:assembleRelease", "--no-daemon"])
        .env("NODE_ENV", "production")
        .env("GRADLE_USER_HOME", &gradle_home)
        .env("ANDROID_USER_HOME", &android_user_home)
        .env("TMPDIR", &tmp)
        .env("TEMP", &tmp)
        .env("TMP", &tmp);

    if let Some(java_home) = java_17_home() {
        command.env("JAVA_HOME", java_home);
    }
    if let Some(android_sdk) = android_sdk_home() {
        command
            .env("ANDROID_HOME", &android_sdk)
            .env("ANDROID_SDK_ROOT", &android_sdk);
        let _ = tx.send(format!(
            "Host native verify ↳ Android SDK {}",
            android_sdk.display()
        ));
    } else {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "Android SDK not found. Set ANDROID_HOME/ANDROID_SDK_ROOT or install Android Studio SDK.",
        ));
    }

    run_host_command_bounded(
        command,
        &project.join(".appforge/host-android-release.log"),
        Duration::from_secs(900),
    )?;

    let apk = project.join("android/app/build/outputs/apk/release/app-release.apk");
    if !apk.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "Android release assemble succeeded but app-release.apk was not found",
        ));
    }
    let _ = tx.send(format!(
        "Host native verify ✓ Android release APK ({} bytes)",
        fs::metadata(&apk)?.len()
    ));
    Ok(())
}

#[cfg(target_os = "macos")]
fn verify_ios_host(project: &Path, tx: &Sender<String>) -> io::Result<()> {
    let ios = project.join("ios");
    ensure_expo_native_dir(project, "ios", &ios, tx)?;

    let workspace = fs::read_dir(&ios)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.extension().is_some_and(|ext| ext == "xcworkspace"))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "iOS xcworkspace not found"))?;

    let scheme = workspace
        .file_stem()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .ok_or_else(|| io::Error::other("could not infer iOS scheme from workspace"))?;

    let _ = tx.send(format!(
        "Host native verify ▶ iOS Release simulator build ({scheme})"
    ));
    let build_command = |derived_data: Option<&Path>| {
        let mut command = Command::new("xcodebuild");
        command
            .current_dir(project)
            .arg("-workspace")
            .arg(&workspace)
            .args([
                "-scheme",
                scheme,
                "-configuration",
                "Release",
                "-sdk",
                "iphonesimulator",
                "-destination",
                "generic/platform=iOS Simulator",
                "CODE_SIGNING_ALLOWED=NO",
            ]);
        if let Some(path) = derived_data {
            command.arg("-derivedDataPath").arg(path);
        }
        command.arg("build").env("NODE_ENV", "production");
        command
    };

    let primary_log = project.join(".appforge/host-ios-release.log");
    if let Err(primary_err) =
        run_host_command_bounded(build_command(None), &primary_log, Duration::from_secs(1200))
    {
        let locked = fs::read_to_string(&primary_log)
            .map(|text| text.contains("database is locked"))
            .unwrap_or(false);
        if !locked {
            return Err(primary_err);
        }

        let isolated = project
            .join(".appforge")
            .join(format!("xcode-derived-data-{}", std::process::id()));
        fs::create_dir_all(&isolated)?;
        let _ = tx.send(format!(
            "Host native verify ↺ Xcode build DB was locked; retrying with isolated DerivedData {}",
            isolated.display()
        ));
        run_host_command_bounded(
            build_command(Some(&isolated)),
            &project.join(".appforge/host-ios-release-retry.log"),
            Duration::from_secs(1200),
        )?;
    }

    let _ = tx.send("Host native verify ✓ iOS Release simulator build".into());
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn verify_ios_host(_project: &Path, tx: &Sender<String>) -> io::Result<()> {
    let _ = tx.send("Host native verify ↷ iOS build skipped on non-macOS host".into());
    Ok(())
}

fn verify_native_host(project: &Path, tx: &Sender<String>) -> io::Result<()> {
    if !expo_project(project) {
        let _ =
            tx.send("Host native verify ↷ non-Expo project; no built-in native verifier".into());
        fs::create_dir_all(project.join(".appforge"))?;
        fs::write(
            project.join(".appforge/native-verify.conf"),
            "host=NOT_APPLICABLE\n",
        )?;
        return Ok(());
    }

    verify_android_host(project, tx)?;
    verify_ios_host(project, tx)?;

    fs::write(
        project.join(".appforge/native-verify.conf"),
        "android=PASS\nios=PASS\n",
    )?;
    Ok(())
}

fn source_file_skipped(project: &Path, path: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(project) else {
        return true;
    };
    [
        ".git",
        ".appforge",
        "node_modules",
        "docs",
        "dist",
        "android/build",
        "android/.gradle",
        "ios/Pods",
        "ios/build",
    ]
    .iter()
    .any(|prefix| relative == Path::new(prefix) || relative.starts_with(prefix))
}

fn collect_source_files(project: &Path, dir: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
    let mut entries = fs::read_dir(dir)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    entries.sort();
    for path in entries {
        if source_file_skipped(project, &path) {
            continue;
        }
        if path.is_dir() {
            collect_source_files(project, &path, files)?;
        } else if path.is_file() {
            files.push(path);
        }
    }
    Ok(())
}

fn source_fingerprint(project: &Path) -> io::Result<String> {
    let mut files = Vec::new();
    collect_source_files(project, project, &mut files)?;
    files.sort();

    let mut hash = 0xcbf29ce484222325u64;
    for path in files {
        let relative = path
            .strip_prefix(project)
            .map_err(|_| io::Error::other("source path escaped project root"))?;
        for byte in relative.to_string_lossy().as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
        hash ^= 0xff;
        hash = hash.wrapping_mul(0x100000001b3);
        for byte in fs::read(&path)? {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    Ok(format!("{hash:016x}"))
}

fn decision_checkpoint_path(project: &Path, gate: &str) -> PathBuf {
    project
        .join(".appforge")
        .join(format!("{gate}-source.fingerprint"))
}

fn save_decision_checkpoint(project: &Path, gate: &str) -> io::Result<()> {
    fs::write(
        decision_checkpoint_path(project, gate),
        format!("{}\n", source_fingerprint(project)?),
    )
}

fn latest_source_modified(project: &Path) -> io::Result<SystemTime> {
    let mut files = Vec::new();
    collect_source_files(project, project, &mut files)?;
    let mut latest = SystemTime::UNIX_EPOCH;
    for path in files {
        if let Ok(modified) = fs::metadata(path).and_then(|metadata| metadata.modified()) {
            if modified > latest {
                latest = modified;
            }
        }
    }
    Ok(latest)
}

fn reusable_decision_checkpoint(project: &Path, gate: &str) -> bool {
    if verify_decision(project, gate).is_err() {
        return false;
    }

    if let Ok(saved) = fs::read_to_string(decision_checkpoint_path(project, gate)) {
        return source_fingerprint(project)
            .map(|current| saved.trim() == current)
            .unwrap_or(false);
    }

    // Migration/recovery path for a PASS written by an older AppForge build
    // that was interrupted after the AI decision but before host verification.
    let decision = project.join(".appforge").join(format!("{gate}-decision"));
    let Ok(decision_time) = fs::metadata(decision).and_then(|metadata| metadata.modified()) else {
        return false;
    };
    latest_source_modified(project)
        .map(|latest| decision_time >= latest)
        .unwrap_or(false)
}

pub fn spawn_stage(
    cfg: Config,
    project_path: PathBuf,
    product_brief: String,
    stage: Stage,
    tx: Sender<WorkerEvent>,
) {
    thread::spawn(move || {
        let stage_lock = match project::acquire_stage_lock(&project_path, stage.id()) {
            Ok(lock) => lock,
            Err(err) => {
                let provider_id = provider_for(&cfg, stage);
                let _ = tx.send(WorkerEvent::Completed {
                    stage,
                    provider: provider_id,
                    result: Err(err.to_string()),
                });
                return;
            }
        };

        let quality_computer = stage == Stage::Quality
            && computer::controller(&cfg).is_some()
            && computer::status().ready();
        let provider_id = if stage == Stage::Publish {
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
            if let Some(text) = aside::cached_research(&project_path, stage.id()) {
                let summary = text
                    .lines()
                    .find(|line| !line.trim().is_empty())
                    .unwrap_or("cached notes");
                let _ = tx.send(WorkerEvent::Log(format!(
                    "Aside ↺ cached {}",
                    truncate(summary, 120)
                )));
            } else {
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
            Stage::Release => Some("release"),
            _ => None,
        };
        let gate_reusable = gate
            .map(|name| reusable_decision_checkpoint(&project_path, name))
            .unwrap_or(false);
        let preparation = (|| -> io::Result<()> {
            if stage == Stage::Quality {
                for relative in [
                    ".appforge/qa-decision",
                    ".appforge/qa-source.fingerprint",
                    ".appforge/ui-decision",
                ] {
                    match std::fs::remove_file(project_path.join(relative)) {
                        Ok(()) => {}
                        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                        Err(e) => return Err(e),
                    }
                }
            }
            if let Some(gate) = gate {
                if !gate_reusable {
                    for file in [
                        project_path.join(format!(".appforge/{gate}-decision")),
                        decision_checkpoint_path(&project_path, gate),
                    ] {
                        match std::fs::remove_file(file) {
                            Ok(()) => {}
                            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                            Err(e) => return Err(e),
                        }
                    }
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
                Stage::Quality => {
                    let mut outcome = if gate_reusable {
                        let _ = log_tx.send(
                            "Quality checkpoint ↺ source unchanged; reusing prior PASS and resuming host gates."
                                .into(),
                        );
                        verify_decision(&project_path, "quality").map_err(|e| e.to_string())
                    } else {
                        let prompt = stage_prompt(stage, &product_brief, &project_path);
                        let _ = log_tx.send(
                            "Quality repair ▶ code/tests/performance checks with the primary provider."
                                .into(),
                        );
                        let mut checked = provider::run_task_bounded(
                            &provider_id,
                            &project_path,
                            &prompt,
                            cfg.auto_mode,
                            cfg.strict_subscription_auth,
                            log_tx.clone(),
                            stage_timeout(stage),
                        )
                        .map_err(|e| e.to_string());

                        if checked.is_ok() {
                            checked = verify_decision(&project_path, "quality")
                                .map_err(|e| e.to_string());
                        }
                        if checked.is_ok() {
                            checked = save_decision_checkpoint(&project_path, "quality")
                                .map_err(|e| format!("quality checkpoint failed: {e}"));
                        }
                        checked
                    };

                    if outcome.is_ok() {
                        let _ = log_tx.send(
                            "Quality host gate ▶ native Android/iOS release verification outside the AI sandbox."
                                .into(),
                        );
                        outcome = verify_native_host(&project_path, &log_tx)
                            .map_err(|e| format!("host native verification failed: {e}"));
                    }

                    if outcome.is_ok() && quality_computer {
                        let smoke = quality_ui_smoke_prompt(&project_path);
                        let _ = log_tx.send(
                            "Quality UI smoke ▶ bounded to 120s; timeout is recorded as SKIPPED."
                                .into(),
                        );
                        match computer::run_agent_task_bounded(
                            &cfg,
                            &project_path,
                            &smoke,
                            log_tx.clone(),
                            std::time::Duration::from_secs(120),
                        ) {
                            Ok(_) => match read_ui_decision(&project_path) {
                                Ok(UiDecision::Blocked) => {
                                    outcome = Err(
                                            "bounded UI smoke found a verified P0/P1 blocker; see docs/04-ui-smoke.md"
                                                .into(),
                                        );
                                }
                                Ok(UiDecision::Pass | UiDecision::Skipped) => {}
                                Err(err) => {
                                    let _ = log_tx.send(format!(
                                        "UI smoke result missing/invalid; treating as SKIPPED: {err}"
                                    ));
                                    let _ = write_ui_skipped(
                                        &project_path,
                                        "Computer-use smoke finished without a valid ui-decision.",
                                    );
                                }
                            },
                            Err(err) => {
                                let _ = log_tx.send(format!("UI smoke non-blocking: {err}"));
                                let _ = write_ui_skipped(
                                    &project_path,
                                    &format!("Computer-use smoke was skipped: {err}"),
                                );
                            }
                        }
                    }
                    outcome
                }
                Stage::Qa => {
                    let mut outcome = if gate_reusable {
                        let _ = log_tx.send(
                            "QA checkpoint ↺ source unchanged; reusing prior PASS and resuming final host gates."
                                .into(),
                        );
                        verify_decision(&project_path, "qa").map_err(|e| e.to_string())
                    } else {
                        let prompt = stage_prompt(stage, &product_brief, &project_path);
                        let mut checked = provider::run_task_bounded(
                            &provider_id,
                            &project_path,
                            &prompt,
                            cfg.auto_mode,
                            cfg.strict_subscription_auth,
                            log_tx.clone(),
                            stage_timeout(stage),
                        )
                        .map_err(|e| e.to_string());

                        if checked.is_ok() {
                            checked =
                                verify_decision(&project_path, "qa").map_err(|e| e.to_string());
                        }
                        if checked.is_ok() {
                            checked = save_decision_checkpoint(&project_path, "qa")
                                .map_err(|e| format!("qa checkpoint failed: {e}"));
                        }
                        checked
                    };

                    // QA is allowed to repair source/config after the earlier Quality
                    // host build. Re-run the native host gate here so Store/Release
                    // evidence always corresponds to the final QA-modified tree.
                    if outcome.is_ok() {
                        let _ = log_tx.send(
                            "QA host gate ▶ re-verifying final Android/iOS native release builds after QA fixes."
                                .into(),
                        );
                        outcome = verify_native_host(&project_path, &log_tx)
                            .map_err(|e| format!("post-QA host native verification failed: {e}"));
                    }

                    // A final bounded UI smoke must happen after QA fixes as well.
                    if outcome.is_ok()
                        && computer::controller(&cfg).is_some()
                        && computer::status().ready()
                    {
                        let smoke = quality_ui_smoke_prompt(&project_path);
                        let _ = log_tx.send(
                            "QA UI smoke ▶ final bounded runtime check after QA repairs.".into(),
                        );
                        match computer::run_agent_task_bounded(
                            &cfg,
                            &project_path,
                            &smoke,
                            log_tx.clone(),
                            std::time::Duration::from_secs(120),
                        ) {
                            Ok(_) => match read_ui_decision(&project_path) {
                                Ok(UiDecision::Blocked) => {
                                    outcome = Err(
                                        "final UI smoke found a verified P0/P1 blocker; see docs/04-ui-smoke.md"
                                            .into(),
                                    );
                                }
                                Ok(UiDecision::Pass | UiDecision::Skipped) => {}
                                Err(err) => {
                                    let _ = log_tx.send(format!(
                                        "Final UI smoke result missing/invalid; treating as SKIPPED: {err}"
                                    ));
                                    let _ = write_ui_skipped(
                                        &project_path,
                                        "Final computer-use smoke finished without a valid ui-decision.",
                                    );
                                }
                            },
                            Err(err) => {
                                let _ = log_tx.send(format!("Final UI smoke non-blocking: {err}"));
                                let _ = write_ui_skipped(
                                    &project_path,
                                    &format!("Final computer-use smoke was skipped: {err}"),
                                );
                            }
                        }
                    }
                    outcome
                }
                Stage::Build => {
                    let passes = build_pass_prompts(&product_brief, &project_path);
                    let mut outcome = Ok(());
                    for (index, prompt) in passes.into_iter().enumerate() {
                        let _ = log_tx.send(format!(
                            "Development pass {}/3 ▶ {}",
                            index + 1,
                            match index {
                                0 => "core scaffold & game loop",
                                1 => "UI, persistence & retention",
                                _ => "dependencies, tests & build verification",
                            }
                        ));
                        if let Err(err) = provider::run_task_bounded(
                            &provider_id,
                            &project_path,
                            &prompt,
                            cfg.auto_mode,
                            cfg.strict_subscription_auth,
                            log_tx.clone(),
                            stage_timeout(stage),
                        ) {
                            outcome = Err(format!("development pass {} failed: {err}", index + 1));
                            break;
                        }
                    }
                    outcome
                }
                Stage::Store | Stage::Release => {
                    let mut prompt = stage_prompt(stage, &product_brief, &project_path);
                    if stage == Stage::Store && cfg.store_draft_upload {
                        prompt.push_str(
                            "\n\nSTORE DRAFT UPLOAD IS ENABLED FOR THIS APPFORGE CONFIG. You MUST write .appforge/store-upload-request.conf exactly as described above so the human can inspect and approve the external draft actions before Publish. Missing this request blocks Publish.",
                        );
                    }
                    provider::run_task_bounded_with_options(
                        &provider_id,
                        &project_path,
                        &prompt,
                        log_tx.clone(),
                        provider::TaskRunOptions {
                            auto: cfg.auto_mode,
                            strict_subscription_auth: cfg.strict_subscription_auth,
                            timeout: stage_timeout(stage),
                            codex_effort: "low",
                        },
                    )
                    .map_err(|e| e.to_string())
                }
                _ => provider::run_task_bounded(
                    &provider_id,
                    &project_path,
                    &stage_prompt(stage, &product_brief, &project_path),
                    cfg.auto_mode,
                    cfg.strict_subscription_auth,
                    log_tx.clone(),
                    stage_timeout(stage),
                )
                .map_err(|e| e.to_string()),
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
        drop(stage_lock);
    });
}

fn build_pass_prompts(brief: &str, project: &Path) -> Vec<String> {
    let safety = format!(
        "You are implementing the generated app in {}. The canonical brief is already in AGENTS.md and .appforge/project.conf. Do not inspect global skills, gstack, ~/.agents, ~/.claude, or ~/.codex instruction frameworks. Work only in this project. Product brief: {}",
        project.display(),
        brief
    );

    vec![
        format!(
            "{safety}\n\nDEVELOPMENT PASS 1/3 — CORE SCAFFOLD & GAME LOOP\nIMPLEMENT NOW. Inspect existing source/config first; continue partial work rather than restarting. Do not read planning documents wholesale. Ensure a runnable app entry/root component exists and wire the core game/domain loop into an actual screen. Complete missing pure domain/generator behavior needed by the main loop. Keep Android/iOS Expo configuration coherent. Do not spend time on store metadata, policy docs, or visual polish in this pass. Run a fast TypeScript/static check if dependencies already exist. Leave the repository in a materially more runnable state."
        ),
        format!(
            "{safety}\n\nDEVELOPMENT PASS 2/3 — UI, PERSISTENCE & RETENTION\nDo not reread docs/01-product.md or docs/02-design.md unless a single concrete detail is missing. Inspect the current code created by pass 1. Complete the required playable UI and connect persistence, retry/undo/hint, progression, Daily/Weekly/streak/achievement/share behavior that the existing product code expects. Fix type inconsistencies you encounter. Add only the minimum programmatic visual assets needed for a coherent portrait mobile MVP. Ensure the root app can launch without placeholder screens."
        ),
        format!(
            "{safety}\n\nDEVELOPMENT PASS 3/3 — DEPENDENCIES, TESTS & BUILD VERIFICATION\nDo not redesign the product. Finish missing package/lock/test/script files. Install dependencies using the repository package manager if needed. Add focused tests for domain/generator/progress behavior and deterministic level validation. Run lint, typecheck, tests, catalog/doctor/export checks that are available, and fix failures rather than merely reporting them. Verify Expo config for both Android and iOS. Update README and docs/03-architecture.md only with concise actual build/run instructions and implementation deltas. This pass must end with concrete command evidence; if a native build cannot run because an external SDK/toolchain is unavailable, record that exact blocker but still make all locally possible checks pass."
        ),
    ]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UiDecision {
    Pass,
    Blocked,
    Skipped,
}

fn quality_ui_smoke_prompt(project: &Path) -> String {
    format!(
        r#"You are a bounded UI smoke-test observer for the generated app at {}.

Do NOT modify source code or install new global tools.

Within the existing project/tooling:
1. Inspect package scripts/config just enough to determine whether a runnable Expo/browser/simulator target is already available.
2. If launch is practical, launch it and use the computer MCP to verify: first render, one core interaction, retry/reset if exposed, and that the UI is not obviously broken on the visible viewport.
3. Stop quickly if the required runtime/simulator cannot be launched from the existing project.
4. Never spend time on market research, store work, Notion, or architecture review.
5. Write docs/04-ui-smoke.md with concise observable evidence.
6. Write .appforge/ui-decision containing exactly one of:
   PASS — the bounded smoke completed without a verified P0/P1 blocker.
   BLOCKED — you directly observed a P0/P1 functional blocker.
   SKIPPED — runtime/simulator/browser was unavailable or the smoke could not be completed.

A missing optional simulator is SKIPPED, not BLOCKED. Do not claim PASS without an actual visible runtime check."#,
        project.display()
    )
}

fn read_ui_decision(project: &Path) -> io::Result<UiDecision> {
    let value = std::fs::read_to_string(project.join(".appforge/ui-decision"))?;
    match value.trim() {
        "PASS" => Ok(UiDecision::Pass),
        "BLOCKED" => Ok(UiDecision::Blocked),
        "SKIPPED" => Ok(UiDecision::Skipped),
        other => Err(io::Error::other(format!(
            "unexpected ui-decision value: {other}"
        ))),
    }
}

fn write_ui_skipped(project: &Path, reason: &str) -> io::Result<()> {
    std::fs::create_dir_all(project.join(".appforge"))?;
    std::fs::create_dir_all(project.join("docs"))?;
    std::fs::write(project.join(".appforge/ui-decision"), "SKIPPED\n")?;
    std::fs::write(
        project.join("docs/04-ui-smoke.md"),
        format!("# UI Smoke Test\n\nDecision: **SKIPPED**\n\n{}\n", reason),
    )
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
    let report = match gate {
        "quality" => "docs/04-quality.md",
        "qa" => "docs/05-qa.md",
        "release" => "docs/08-release.md",
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("unknown decision gate: {gate}"),
            ))
        }
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

fn verify_native_evidence(project: &Path) -> io::Result<()> {
    let evidence = std::fs::read_to_string(project.join(".appforge/native-verify.conf"))?;
    if evidence
        .lines()
        .any(|line| line.trim() == "host=NOT_APPLICABLE")
    {
        return Ok(());
    }
    let android = evidence.lines().any(|line| line.trim() == "android=PASS");
    let ios = evidence.lines().any(|line| line.trim() == "ios=PASS");
    if android && ios {
        Ok(())
    } else {
        Err(io::Error::other(
            "final native verification is missing Android and/or iOS PASS evidence",
        ))
    }
}

fn verify_gates(project: &Path) -> io::Result<()> {
    verify_decision(project, "quality")?;
    verify_decision(project, "qa")?;
    verify_native_evidence(project)
}

fn stage_prompt(stage: Stage, _brief: &str, project: &Path) -> String {
    let research_note = if uses_aside(stage) {
        format!(
            " Browser research for this stage is already supplied at docs/aside/{}.md when that file exists. Treat it as the current research input and DO NOT repeat the same market, competitor, policy, or documentation searches. Use web search only for a concrete fact that is missing, stale, or contradictory, and then return immediately to the requested artifact/code work.",
            stage.id()
        )
    } else {
        String::new()
    };
    let common = format!(
        "You are working inside {}. The canonical product brief is already in AGENTS.md and .appforge/project.conf; do not ask for it again. This AppForge stage prompt is complete: DO NOT inspect, invoke, or follow global agent/skill frameworks under ~/.agents, ~/.claude, ~/.codex, gstack, or unrelated home-directory instructions. Use only this generated project's AGENTS.md, project files, and stage-relevant docs. Read focused files rather than concatenating every document. Keep work production-oriented, cross-platform, measurable, and reproducible. Never inspect or extract OAuth tokens, browser cookies, keychains, or another tool's credential files. Use existing authenticated CLIs only. Do not publish anything, upload store artifacts, change account settings, or submit a release in this stage; only the separately configured Publish stage can publish policy pages or upload approved drafts.{}",
        project.display(),
        research_note
    );

    let task = match stage {
        Stage::Plan => {
            "Act as the PRIMARY ORCHESTRATOR. Do not implement the app yet. Define product scope, core loop, MVP/non-goals, monetization assumptions if relevant, target users, platform constraints, acceptance criteria, and a compact task DAG. Write docs/01-product.md and keep it under roughly 1,500 words. Also decide whether Expo/React Native + TypeScript is sufficient and write a concise docs/03-architecture.md under roughly 900 words. Do not inflate these documents with exhaustive prose; downstream workers need compact execution handoffs."
        }
        Stage::Design => {
            "Act as the DESIGN SUB-AGENT. Read docs/01-product.md and docs/aside/design.md if present. Produce a concrete mobile-first game/app UX: screen map, controls, feedback, states, typography/color guidance, asset list, accessibility, onboarding, retention loop, and screenshot plan. Write docs/02-design.md and keep it under roughly 1,500 words. Prefer compact tables/checklists over long narrative. You may add lightweight wireframe/spec files, but do not replace the product scope."
        }
        Stage::Build => {
            "Act as the DEVELOPMENT SUB-AGENT. IMPLEMENT NOW; do not spend the turn rewriting or re-summarizing planning documents. First inspect the existing source tree and package/config files. Read only the sections of docs/01-product.md, docs/02-design.md, and docs/03-architecture.md needed to resolve a concrete implementation question; never concatenate those full documents into one command. Continue any partial implementation already present. Prioritize this checklist in order: (1) runnable app entry/root UI, (2) core game/domain loop wired to UI, (3) persistence and required MVP retention features, (4) deterministic level validation/tests, (5) lint/typecheck/test scripts and lockfile, (6) Android/iOS Expo configuration. Prefer Expo + React Native + TypeScript unless architecture explicitly rejects it. Run available install/check/test/export commands and fix failures. Update docs/03-architecture.md only with short actual-decision deltas. The stage is incomplete unless real source plus executable verification exists."
        }
        Stage::Quality => {
            r#"Act as the FUNCTIONAL + PERFORMANCE REPAIR AGENT. Your job is not to write a favorable review; find weak functionality and measurable performance risks, then fix them.

1. Detect the actual stack and package manager from lockfiles/config.
2. Run the strongest available lint, typecheck, unit/integration tests, production build/export, framework doctor, and static analysis without weakening existing gates.
3. Exercise everything that can be verified from code, tests, framework tooling, exported bundles, and deterministic scripts. Do NOT wait for or invoke GUI/computer-use tooling in this pass; AppForge runs a separate bounded UI smoke test after this code repair passes.
4. Do NOT run Android Gradle release assembly or Xcode Release compilation from this AI sandbox. AppForge runs those native build gates directly from the host after your code-quality decision. A missing native-build result inside this AI turn is therefore PENDING HOST VERIFICATION, not a reason by itself to mark this code-quality decision BLOCKED.
5. Measure what the stack makes practical: startup/build/export time, bundle/asset size, long-running tasks, excessive rerenders, unbounded lists, image/media waste, synchronous storage/network bottlenecks, memory/resource leaks, and obvious frame-rate risks. Do not invent benchmark numbers when tooling cannot measure them.
6. Prioritize P0/P1 functional defects and user-visible performance regressions. Fix verified issues in the code, then rerun the relevant checks.
7. Do not replace real checks with mocks merely to pass. Do not delete features to improve a metric.
8. Write docs/04-quality.md containing before/after evidence, commands or runtime checks performed, fixes made, unresolved risks, and a clear PASS/BLOCKED CODE-QUALITY decision. PASS here means the code/tests/export checks under your control are release-clean; AppForge will still block later if its host-native or UI gates fail."#
        }
        Stage::Qa => {
            "Act as the PRIMARY REVIEWER after the quality-repair stage. Inspect the complete implementation and git diff. Run available lint, typecheck, unit/integration tests, config checks, and JS/export checks again. Fix correctness, state-management, UX-blocking, security/privacy, accessibility, and mobile compatibility issues you can verify. Recheck issues recorded in docs/04-quality.md and docs/04-ui-smoke.md. The native artifacts from the earlier Quality stage are PRE-QA evidence only: if you modify source/config, they become stale by design. DO NOT mark QA BLOCKED merely because those pre-QA artifacts are stale, and DO NOT rerun Gradle/Xcode native release builds from this AI sandbox. After your QA decision, AppForge itself will rebuild Android/iOS from the final QA-modified tree and will fail the stage if those host builds fail. Write docs/05-qa.md with commands run, results, fixes, remaining code-level risks, and a PASS/BLOCKED code/static QA decision. Do not lower quality gates just to make checks green."
        }
        Stage::Store => {
            r#"Act as the STORE/POLICY SUB-AGENT. This is a DOCUMENTATION stage, not another engineering review. Quality and QA have already passed.

Work quickly and write the required artifacts early. Do NOT rerun npm install, tests, Expo Doctor, exports, Gradle, Xcode, dependency audits, market research, or browser searches. Do NOT inspect node_modules, native build output trees, the full lockfile, or unrelated source. Use only these focused inputs: package.json, app.json, AGENTS.md, docs/04-quality.md, docs/05-qa.md, docs/aside/store.md if present, and targeted rg searches in src/ for network/auth/analytics/ads/storage/share behavior when a privacy fact is unclear.

Create docs/06-store.md with concise en-US Google Play and Apple App Store listing copy, exact package/bundle identifiers, category, privacy/data disclosure, permissions rationale, age/content rating inputs, screenshot/icon plan, verified artifact locations, support fields, and manual approval points.

Create docs/policies/privacy-policy.md from VERIFIED app behavior. Also create docs/policies/terms.md and docs/policies/support-and-data-deletion.md. Keep each document concise and publication-ready.

Use PUBLIC_PRIVACY_POLICY_URL, PUBLIC_TERMS_URL, and PUBLIC_SUPPORT_URL placeholders in docs/06-store.md for later Notion/public URL replacement.

If store draft upload is enabled, write .appforge/store-upload-request.conf exactly as requested by the surrounding prompt and include only artifacts that actually exist and are hashed.

Do not submit to a store, create irreversible identifiers, accept legal agreements, change pricing, or invent signing credentials. Finish after the required documents are written."#
        }
        Stage::Publish => {
            "Computer-use publication is handled by the dedicated CUA stage."
        }
        Stage::Release => {
            "Act as the PRIMARY RELEASE ORCHESTRATOR for a concise documentation-only final gate. Quality/QA/native builds already passed. Do not rerun installs, tests, exports, Gradle, Xcode, dependency audits, browser research, or inspect node_modules. Read only docs/04-quality.md, docs/05-qa.md, docs/06-store.md, docs/07-publish.md when present, package.json/app.json, and existing .appforge stage/native status files. Verify version/build identifiers, CI/build commands, signing placeholders, artifact paths, policy/store metadata consistency, and remaining human gates. Write a compact docs/08-release.md with READY/BLOCKED and exact remaining manual actions. Also write .appforge/release-decision containing exactly PASS when docs/08-release.md is READY, or BLOCKED when docs/08-release.md is BLOCKED. App Store / Play Store final review submission and production rollout remain explicit human approval steps."
        }
    };
    let gate = match stage {
        Stage::Quality => "quality",
        Stage::Qa => "qa",
        Stage::Release => "release",
        _ => "",
    };
    let decision = if gate.is_empty() {
        String::new()
    } else if stage == Stage::Quality {
        "\nWrite .appforge/quality-decision containing exactly PASS or BLOCKED on one line. PASS requires the code-quality checks that are actually assigned to this AI pass to succeed, no unresolved code-level P0/P1 blocker, and a durable docs/04-quality.md report. Android Gradle/Xcode native release compilation and GUI runtime evidence are separate AppForge host/UI gates after this decision; their absence inside this sandbox must not by itself make this code-quality decision BLOCKED. Do not reuse a prior decision.".to_string()
    } else if stage == Stage::Qa {
        "\nWrite .appforge/qa-decision containing exactly PASS or BLOCKED on one line. PASS requires the code/static QA checks assigned to this AI pass to succeed, no unresolved code-level P0/P1 blocker, and a durable docs/05-qa.md report. Pre-QA Android/iOS artifacts may be stale after your fixes; that is expected and must not by itself cause BLOCKED because AppForge immediately rebuilds final native artifacts on the host after this decision. Missing or failed code/static checks still mean BLOCKED. Do not reuse a prior decision.".to_string()
    } else if stage == Stage::Release {
        "\nWrite .appforge/release-decision containing exactly PASS or BLOCKED on one line. PASS is allowed only when docs/08-release.md says READY and no unresolved release blocker remains. If docs/08-release.md says BLOCKED for any reason, this decision MUST be BLOCKED. Do not reuse a prior decision.".to_string()
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

fn stage_complete(project_path: &Path, stage: Stage) -> bool {
    if !project::stage_done(project_path, stage.id()) {
        return false;
    }
    match stage {
        Stage::Quality => {
            verify_decision(project_path, "quality").is_ok()
                && verify_native_evidence(project_path).is_ok()
        }
        Stage::Qa => {
            verify_decision(project_path, "qa").is_ok()
                && verify_native_evidence(project_path).is_ok()
        }
        Stage::Release => verify_decision(project_path, "release").is_ok(),
        _ => true,
    }
}

pub fn run_all(cfg: Config, project: PathBuf, brief: String) -> io::Result<()> {
    let pending = Stage::all()
        .iter()
        .copied()
        .filter(|stage| !stage_complete(&project, *stage))
        .collect::<Vec<_>>();
    if pending.is_empty() {
        println!("All AppForge stages are already complete.");
        return Ok(());
    }
    for stage in Stage::all() {
        if stage_complete(&project, *stage) {
            println!("{} ↺ already complete; skipping", stage.title());
        }
    }
    run_stages(cfg, project, brief, &pending)
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

    #[test]
    fn qa_prompt_delegates_final_native_rebuild_to_host_gate() {
        let prompt = stage_prompt(Stage::Qa, "brief", Path::new("/tmp/project"));
        assert!(prompt.contains("PRE-QA evidence only"));
        assert!(prompt.contains("AppForge itself will rebuild Android/iOS"));
        assert!(prompt.contains("must not by itself cause BLOCKED"));
    }

    #[test]
    fn decision_checkpoint_reuses_only_unchanged_source() {
        let root =
            std::env::temp_dir().join(format!("appforge-checkpoint-test-{}", std::process::id()));
        let project = project::create(&root, "checkpoint test").unwrap();
        fs::write(project.join("docs/05-qa.md"), "QA evidence").unwrap();
        fs::write(project.join(".appforge/qa-decision"), "PASS\n").unwrap();

        // Legacy/interrupted PASS files without a fingerprint are reusable
        // only while no source file is newer than the decision.
        assert!(reusable_decision_checkpoint(&project, "qa"));
        save_decision_checkpoint(&project, "qa").unwrap();
        assert!(reusable_decision_checkpoint(&project, "qa"));

        fs::write(project.join("App.tsx"), "changed source").unwrap();
        assert!(!reusable_decision_checkpoint(&project, "qa"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn release_status_requires_explicit_pass_decision() {
        let root =
            std::env::temp_dir().join(format!("appforge-release-test-{}", std::process::id()));
        let project = project::create(&root, "release gate test").unwrap();
        project::mark_stage(&project, "release", true, "legacy done").unwrap();
        fs::write(project.join("docs/08-release.md"), "Decision: **BLOCKED**").unwrap();

        assert!(!stage_complete(&project, Stage::Release));

        fs::write(project.join(".appforge/release-decision"), "BLOCKED\n").unwrap();
        assert!(!stage_complete(&project, Stage::Release));

        fs::write(project.join("docs/08-release.md"), "Decision: **READY**").unwrap();
        fs::write(project.join(".appforge/release-decision"), "PASS\n").unwrap();
        assert!(stage_complete(&project, Stage::Release));

        let _ = fs::remove_dir_all(root);
    }
}
