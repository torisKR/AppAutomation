use std::collections::VecDeque;
use std::io::{self, IsTerminal, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use crate::config::Config;
use crate::orchestrator::{self, Stage, WorkerEvent};
use crate::project;
use crate::provider;
use crate::terminal::{self, Action, Input};

static MANUAL_REQUESTED: AtomicBool = AtomicBool::new(false);

#[cfg(unix)]
const SIGINT: i32 = 2;

#[cfg(unix)]
unsafe extern "C" {
    fn signal(sig: i32, handler: usize) -> usize;
}

#[cfg(unix)]
extern "C" fn handle_sigint(_: i32) {
    MANUAL_REQUESTED.store(true, Ordering::SeqCst);
}

#[cfg(unix)]
struct SignalGuard(usize);

#[cfg(unix)]
impl Drop for SignalGuard {
    fn drop(&mut self) {
        unsafe {
            signal(SIGINT, self.0);
        }
    }
}

#[cfg(unix)]
fn install_sigint_handler() -> io::Result<SignalGuard> {
    let previous = unsafe { signal(SIGINT, handle_sigint as *const () as usize) };
    if previous == usize::MAX {
        return Err(io::Error::last_os_error());
    }
    Ok(SignalGuard(previous))
}

#[cfg(not(unix))]
fn install_sigint_handler() -> io::Result<()> {
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StepState {
    Pending,
    Running,
    Done,
    Failed,
}

struct State {
    cfg: Config,
    project: Option<std::path::PathBuf>,
    brief: String,
    stage_index: usize,
    steps: Vec<StepState>,
    running: bool,
    auto: bool,
    paused: bool,
    logs: VecDeque<String>,
    last_error: Option<String>,
    providers: Vec<provider::ProviderInfo>,
    restart_from_stage: Option<usize>,
}

impl State {
    fn new(cfg: Config) -> Self {
        Self {
            auto: cfg.auto_mode,
            cfg,
            project: None,
            brief: String::new(),
            stage_index: 0,
            steps: vec![StepState::Pending; Stage::all().len()],
            running: false,
            paused: false,
            logs: VecDeque::with_capacity(300),
            last_error: None,
            providers: Vec::new(),
            restart_from_stage: None,
        }
    }

    fn push_log(&mut self, line: impl Into<String>) {
        if self.logs.len() >= 300 {
            self.logs.pop_front();
        }
        self.logs.push_back(line.into());
    }

    fn complete(&self) -> bool {
        self.project.is_some() && self.stage_index >= Stage::all().len()
    }
}

pub fn run(cfg: Config) -> io::Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(io::Error::other(
            "interactive TUI requires a terminal; use appforge new/run for headless mode",
        ));
    }

    MANUAL_REQUESTED.store(false, Ordering::SeqCst);
    #[cfg(unix)]
    let _signal = install_sigint_handler()?;
    #[cfg(not(unix))]
    install_sigint_handler()?;
    let terminal = terminal::Session::enter()?;
    let (input_tx, input_rx) = mpsc::channel();
    spawn_command_reader(input_tx.clone(), terminal.mouse_enabled());
    // CLI auth diagnostics can block; never run them inside a redraw.
    thread::spawn(move || {
        let _ = input_tx.send(InputEvent::Providers(provider::detected()));
    });

    let (worker_tx, worker_rx) = mpsc::channel::<WorkerEvent>();
    let mut state = State::new(cfg);
    let mut input = Input::default();
    let mut buttons = Vec::new();
    let mut input_closed = false;
    state.push_log("AppForge ready. Type a product/game brief and press Enter.");
    state.push_log("Controls: auto · manual · run · new <brief> · quit");
    state.push_log("Ctrl+C switches to manual mode; active work is not cancelled.");
    if !terminal.mouse_enabled() {
        state.push_log("Mouse controls unavailable on this platform; use typed commands.");
    }

    loop {
        if MANUAL_REQUESTED.swap(false, Ordering::SeqCst) {
            input.clear();
            handle_command(&mut state, "manual".into(), &worker_tx)?;
            state.push_log("Ctrl+C → MANUAL mode requested.");
        }

        drain_worker_events(&mut state, &worker_rx);
        while let Ok(event) = input_rx.try_recv() {
            let actions = match event {
                InputEvent::Bytes(bytes) => input.feed(&bytes),
                InputEvent::Line(line) => vec![Action::Command(line)],
                InputEvent::Providers(providers) => {
                    state.providers = providers;
                    Vec::new()
                }
                InputEvent::Closed => {
                    input_closed = true;
                    // Losing input must never continue unattended execution.
                    handle_command(&mut state, "manual".into(), &worker_tx)?;
                    if !state.running {
                        return Ok(());
                    }
                    state.push_log("Terminal input closed; waiting for the active worker.");
                    Vec::new()
                }
                InputEvent::Error(err) => return Err(err),
            };
            for action in actions {
                let command = match action {
                    Action::Command(line) => Some(line),
                    Action::Manual => Some("manual".into()),
                    Action::Click { column, row } => buttons.iter().find_map(|button: &Button| {
                        button.hit(column, row).then(|| button.command.to_string())
                    }),
                };
                if let Some(command) = command {
                    if handle_command(&mut state, command, &worker_tx)? {
                        return Ok(());
                    }
                }
            }
        }

        if input_closed && !state.running {
            return Ok(());
        }

        if state.auto
            && !state.paused
            && !state.running
            && state.project.is_some()
            && state.stage_index < Stage::all().len()
        {
            start_current_stage(&mut state, &worker_tx);
        }

        buttons = draw(&state, &input.text(), terminal.mouse_enabled())?;
        thread::sleep(Duration::from_millis(150));
    }
}

fn handle_command(
    state: &mut State,
    line: String,
    worker_tx: &Sender<WorkerEvent>,
) -> io::Result<bool> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        if state.project.is_some() && !state.running && !state.complete() {
            state.auto = false;
            state.paused = true;
            start_current_stage(state, worker_tx);
        }
        return Ok(false);
    }

    match trimmed {
        "auto" | "a" => {
            state.auto = true;
            state.paused = false;
            state.push_log("Mode → AUTO");
        }
        "manual" | "pause" | "m" => {
            state.auto = false;
            state.paused = true;
            state.push_log("Mode → MANUAL");
        }
        "run" | "r" => {
            if state.project.is_some() && !state.running && !state.complete() {
                state.auto = false;
                state.paused = true;
                start_current_stage(state, worker_tx);
            }
        }
        "quit" | "q" | "exit" => {
            // A pending quit also prevents AUTO from launching another worker.
            state.auto = false;
            state.paused = true;
            if !state.running {
                return Ok(true);
            }
            state
                .push_log("A worker is still running; switch to manual and wait for it to finish.");
        }
        _ if trimmed.starts_with("new ") => {
            if state.running {
                state.push_log("Cannot create a new project while a worker is running.");
            } else {
                create_project(state, trimmed.trim_start_matches("new ").trim())?;
            }
        }
        _ => {
            if state.project.is_none() || state.complete() {
                create_project(state, trimmed)?;
            } else if let Some(project_path) = state.project.clone() {
                project::update_brief(&project_path, trimmed)?;
                state.brief = trimmed.to_string();
                state.last_error = None;
                state.steps.fill(StepState::Pending);
                if state.running {
                    state.restart_from_stage = Some(0);
                    state.push_log(
                        "Product brief updated while a worker is active. The current worker will finish, then the pipeline will restart from Product plan with the new brief.",
                    );
                } else {
                    state.stage_index = 0;
                    state.restart_from_stage = None;
                    state.push_log(
                        "Product brief updated. The pipeline was reset to Product plan; press Run or Auto to continue.",
                    );
                }
            }
        }
    }
    Ok(false)
}

fn create_project(state: &mut State, brief: &str) -> io::Result<()> {
    if brief.trim().is_empty() {
        return Ok(());
    }
    let path = project::create(&state.cfg.projects_dir, brief)?;
    state.project = Some(path.clone());
    state.brief = brief.to_string();
    state.stage_index = 0;
    state.steps = vec![StepState::Pending; Stage::all().len()];
    state.last_error = None;
    state.paused = !state.auto;
    state.push_log(format!("Project created: {}", path.display()));
    Ok(())
}

fn start_current_stage(state: &mut State, worker_tx: &Sender<WorkerEvent>) {
    if state.running || state.stage_index >= Stage::all().len() {
        return;
    }
    let Some(project_path) = state.project.clone() else {
        return;
    };
    let stage = Stage::all()[state.stage_index];
    state.running = true;
    state.steps[state.stage_index] = StepState::Running;
    let provider_id = if stage == Stage::Publish {
        crate::computer::controller(&state.cfg)
            .unwrap_or_else(|| orchestrator::provider_for(&state.cfg, stage))
    } else {
        orchestrator::provider_for(&state.cfg, stage)
    };
    state.push_log(format!(
        "Starting {} with {}",
        stage.title(),
        provider::label(&provider_id)
    ));

    let mut run_cfg = state.cfg.clone();
    run_cfg.auto_mode = state.auto;
    orchestrator::spawn_stage(
        run_cfg,
        project_path,
        state.brief.clone(),
        stage,
        worker_tx.clone(),
    );
}

fn drain_worker_events(state: &mut State, rx: &Receiver<WorkerEvent>) {
    while let Ok(event) = rx.try_recv() {
        match event {
            WorkerEvent::Log(line) => state.push_log(line),
            WorkerEvent::Completed {
                stage,
                provider,
                result,
            } => {
                state.running = false;
                let idx = Stage::all()
                    .iter()
                    .position(|s| *s == stage)
                    .unwrap_or(state.stage_index);
                if let Some(restart) = state.restart_from_stage.take() {
                    state.steps.fill(StepState::Pending);
                    state.stage_index = restart;
                    state.last_error = None;
                    state.push_log(
                        "Previous worker result discarded because the product brief changed. Restarting from Product plan.",
                    );
                    continue;
                }
                match result {
                    Ok(()) => {
                        if idx < state.steps.len() {
                            state.steps[idx] = StepState::Done;
                        }
                        state.last_error = None;
                        state.stage_index = idx.saturating_add(1);
                        state.push_log(format!(
                            "{} ✓ by {}",
                            stage.title(),
                            provider::label(&provider)
                        ));
                        if state.complete() {
                            state.push_log(
                                "Pipeline complete. Review docs/08-release.md and docs/07-publish.md before final store submission.",
                            );
                        }
                    }
                    Err(err) => {
                        if idx < state.steps.len() {
                            state.steps[idx] = StepState::Failed;
                        }
                        state.auto = false;
                        state.paused = true;
                        state.last_error = Some(err.clone());
                        state.push_log(format!("{} ✗ {err}", stage.title()));
                    }
                }
            }
        }
    }
}

enum InputEvent {
    Bytes(Vec<u8>),
    Line(String),
    Providers(Vec<provider::ProviderInfo>),
    Closed,
    Error(io::Error),
}

fn spawn_command_reader(tx: Sender<InputEvent>, immediate: bool) {
    thread::spawn(move || loop {
        let result = if immediate {
            let mut bytes = [0; 256];
            io::stdin().read(&mut bytes).map(|n| {
                if n == 0 {
                    InputEvent::Closed
                } else {
                    InputEvent::Bytes(bytes[..n].to_vec())
                }
            })
        } else {
            let mut line = String::new();
            io::stdin().read_line(&mut line).map(|n| {
                if n == 0 {
                    InputEvent::Closed
                } else {
                    InputEvent::Line(line)
                }
            })
        };
        let event = match result {
            Ok(event) => event,
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) => InputEvent::Error(err),
        };
        let closed = matches!(event, InputEvent::Closed | InputEvent::Error(_));
        if tx.send(event).is_err() || closed {
            break;
        }
    });
}

#[derive(Debug)]
struct Button {
    command: &'static str,
    first: usize,
    last: usize,
}

impl Button {
    fn hit(&self, column: usize, row: usize) -> bool {
        row == 1 && (self.first..=self.last).contains(&column)
    }
}

fn controls(width: usize) -> (String, Vec<Button>) {
    let labels = if width >= 29 {
        ["[Manual]", "[Auto]", "[Run]", "[Quit]"]
    } else {
        ["[M]", "[A]", "[R]", "[Q]"]
    };
    let mut text = String::new();
    let mut buttons = Vec::new();
    for (label, command) in labels.into_iter().zip(["manual", "auto", "run", "quit"]) {
        if !text.is_empty() {
            text.push(' ');
        }
        let first = text.len() + 1;
        text.push_str(label);
        if text.len() <= width {
            buttons.push(Button {
                command,
                first,
                last: text.len(),
            });
        }
    }
    (text, buttons)
}

fn draw(state: &State, input: &str, mouse: bool) -> io::Result<Vec<Button>> {
    let (columns, height) = terminal::size();
    // Leave the last column unused to avoid autowrap on a full-width line.
    let width = columns.saturating_sub(1);
    if width < 20 || height < 8 {
        print!(
            "\x1b[2J\x1b[H{}",
            truncate("Resize terminal to 21x8 or larger", width)
        );
        io::stdout().flush()?;
        return Ok(Vec::new());
    }
    let rows = height.saturating_sub(6).min(18);
    let content = width - 4;
    let left = (content / 4).min(29);
    let mid = (content / 3).min(35);
    let right = content - left - mid;
    let (control_text, buttons) = controls(width);
    let mut frame = String::from("\x1b[2J\x1b[H");
    frame.push_str(&pad(&control_text, width));
    frame.push('\n');
    frame.push_str(&pad(
        &format!(
            " APPFORGE │ {} │ {}",
            mode_text(state),
            state
                .project
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "no project".into())
        ),
        width,
    ));
    frame.push('\n');
    frame.push_str(&"─".repeat(width));
    frame.push('\n');

    let providers = provider_lines(state, left, rows);
    let pipeline = pipeline_lines(state, mid, rows);
    let logs = log_lines(state, right, rows);
    for row in 0..rows {
        frame.push_str(&format!(
            "│{}│{}│{}│\n",
            pad(providers.get(row).map(String::as_str).unwrap_or(""), left),
            pad(pipeline.get(row).map(String::as_str).unwrap_or(""), mid),
            pad(logs.get(row).map(String::as_str).unwrap_or(""), right)
        ));
    }
    frame.push_str(&"─".repeat(width));
    frame.push('\n');
    let hint = if state.project.is_none() || state.complete() {
        "Brief > type an idea and press Enter (Ctrl+C = manual)"
    } else {
        "Command > auto | manual | run | quit (Ctrl+C = manual)"
    };
    frame.push_str(&truncate(hint, width));
    frame.push_str("\n> ");
    frame.push_str(&truncate(input, width.saturating_sub(2)));
    print!("{frame}");
    io::stdout().flush()?;
    Ok(if mouse { buttons } else { Vec::new() })
}

fn mode_text(state: &State) -> &'static str {
    if state.running {
        "RUNNING"
    } else if state.complete() {
        "DONE"
    } else if state.auto && !state.paused {
        "AUTO"
    } else {
        "MANUAL"
    }
}

fn provider_lines(state: &State, width: usize, rows: usize) -> Vec<String> {
    let mut out = vec![" PROVIDERS".into(), "".into()];
    if state.providers.is_empty() {
        out.push("Checking CLIs…".into());
    }
    for info in &state.providers {
        let role = if state.cfg.primary == info.id {
            "P"
        } else if state.cfg.secondary.iter().any(|v| v == info.id) {
            "S"
        } else {
            "-"
        };
        out.push(truncate(
            &format!(
                "{} [{}] {}",
                if info.installed { "●" } else { "○" },
                role,
                info.label
            ),
            width,
        ));
        out.push(truncate(&format!("  {}", info.auth_summary), width));
    }
    out.push(String::new());
    out.push(truncate(
        &format!(
            "strict auth: {}",
            if state.cfg.strict_subscription_auth {
                "ON"
            } else {
                "OFF"
            }
        ),
        width,
    ));
    out.push(truncate(
        &format!(
            "aside: {}",
            if state.cfg.aside_enabled { "ON" } else { "OFF" }
        ),
        width,
    ));
    out.truncate(rows);
    out
}

fn pipeline_lines(state: &State, width: usize, rows: usize) -> Vec<String> {
    let mut out = vec![" PIPELINE".into(), "".into()];
    let capacity = rows.saturating_sub(2);
    let first = state
        .stage_index
        .min(Stage::all().len().saturating_sub(capacity));
    for (idx, stage) in Stage::all().iter().enumerate().skip(first).take(capacity) {
        let marker = match state.steps.get(idx).copied().unwrap_or(StepState::Pending) {
            StepState::Pending => "·",
            StepState::Running => "▶",
            StepState::Done => "✓",
            StepState::Failed => "✗",
        };
        out.push(truncate(&format!("{marker} {}", stage.title()), width));
    }
    if let Some(err) = &state.last_error {
        out.push(String::new());
        out.push(truncate(&format!("ERROR: {err}"), width));
    }
    out.truncate(rows);
    out
}

fn log_lines(state: &State, width: usize, rows: usize) -> Vec<String> {
    let mut out = vec![" LIVE WORKERS".into(), "".into()];
    let take = rows.saturating_sub(2);
    let start = state.logs.len().saturating_sub(take);
    for line in state.logs.iter().skip(start) {
        out.push(truncate(line, width));
    }
    out.truncate(rows);
    out
}

fn truncate(value: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let safe = value
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>();
    let value = safe.as_str();
    if value.chars().count() <= width {
        value.to_string()
    } else if width == 1 {
        "…".into()
    } else {
        value.chars().take(width - 1).collect::<String>() + "…"
    }
}

fn pad(value: &str, width: usize) -> String {
    let value = truncate(value, width);
    let len = value.chars().count();
    format!("{value}{}", " ".repeat(width.saturating_sub(len)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_pipeline_view_tracks_current_stage() {
        let mut state = State::new(Config::default());
        state.stage_index = 7;
        let lines = pipeline_lines(&state, 80, 5);
        assert!(lines.iter().any(|line| line.contains("08 Release")));
        assert_eq!(pipeline_lines(&state, 80, 10).len(), 10);
    }

    #[test]
    fn truncate_is_bounded() {
        assert_eq!(truncate("abcdef", 4), "abc…");
    }

    #[test]
    fn pad_fills_target_width() {
        assert_eq!(pad("ab", 4), "ab  ");
    }
}
