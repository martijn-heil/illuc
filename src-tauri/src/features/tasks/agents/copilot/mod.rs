use crate::features::tasks::agents::{Agent, AgentCallbacks, AgentRuntime, ChildHandle};
use crate::features::tasks::TaskStatus;
use crate::utils::screen::Screen;
#[cfg(target_os = "windows")]
use crate::utils::windows::build_wsl_command;
#[cfg(target_os = "windows")]
use crate::utils::windows::build_wsl_process_command;
#[cfg(target_os = "windows")]
use crate::utils::windows::to_wsl_path;
use anyhow::Context;
use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use parking_lot::Mutex;
use portable_pty::{native_pty_system, PtySize};
#[cfg(not(target_os = "windows"))]
use portable_pty::CommandBuilder;
use std::fs;
use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const DEFAULT_ROWS: u16 = 40;
const DEFAULT_COLS: u16 = 80;

const COPILOT_SESSION_DIR: &str = ".copilot/session-state";
const COPILOT_LEGACY_SESSION_DIR: &str = ".copilot/history-session-state";

#[derive(Clone)]
pub struct CopilotAgent {
    state: Arc<Mutex<CopilotAgentState>>,
}

struct CopilotAgentState {
    screen: Screen,
    last_output: Option<Instant>,
    last_status: Option<TaskStatus>,
}

impl Default for CopilotAgent {
    fn default() -> Self {
        Self {
            state: Arc::new(Mutex::new(CopilotAgentState {
                screen: Screen::new(DEFAULT_ROWS as usize, DEFAULT_COLS as usize),
                last_output: None,
                last_status: None,
            })),
        }
    }
}

struct SessionCandidate {
    session_id: String,
    timestamp: Option<DateTime<Utc>>,
}

fn resolve_session_cwd(worktree_path: &Path) -> anyhow::Result<String> {
    let canonical = fs::canonicalize(worktree_path)
        .with_context(|| format!("failed to resolve cwd {}", worktree_path.display()))?;
    #[cfg(target_os = "windows")]
    if let Some(wsl_path) = to_wsl_path(&canonical) {
        return Ok(wsl_path);
    }
    Ok(canonical.to_string_lossy().to_string())
}

#[cfg(not(target_os = "windows"))]
fn resolve_home_dir() -> anyhow::Result<std::path::PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(std::path::PathBuf::from)
        .context("failed to resolve home directory")
}

#[cfg(target_os = "windows")]
fn resolve_wsl_home_dir(worktree_path: &Path) -> anyhow::Result<std::path::PathBuf> {
    let output = build_wsl_process_command(
        worktree_path,
        "bash",
        &["-lc", "wslpath -w \"$HOME\""],
    )
    .output()
    .context("failed to query WSL home directory")?;
    if !output.status.success() {
        return Err(anyhow::anyhow!("failed to query WSL home directory"));
    }
    let home = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if home.is_empty() {
        return Err(anyhow::anyhow!("WSL home directory is empty"));
    }
    Ok(std::path::PathBuf::from(home))
}

fn parse_timestamp(value: &str) -> Option<DateTime<Utc>> {
    let mut normalized = value.trim().to_string();
    if normalized.ends_with('Z') {
        normalized = format!("{}+00:00", normalized.trim_end_matches('Z'));
    }
    if let Ok(parsed) = DateTime::parse_from_rfc3339(&normalized) {
        return Some(parsed.with_timezone(&Utc));
    }
    if let Ok(naive) = NaiveDateTime::parse_from_str(&normalized, "%Y-%m-%dT%H:%M:%S%.f") {
        return Some(Utc.from_utc_datetime(&naive));
    }
    None
}

fn parse_session_file(path: &Path, desired_cwd: &str) -> Option<SessionCandidate> {
    let data = fs::read_to_string(path).ok()?;
    if !data.contains(desired_cwd) {
        return None;
    }

    let mut session_id: Option<String> = None;
    let mut latest_timestamp: Option<DateTime<Utc>> = None;

    for line in data.lines() {
        let value: serde_json::Value = match serde_json::from_str(line) {
            Ok(value) => value,
            Err(_) => continue,
        };
        if session_id.is_none() {
            if value.get("type").and_then(|value| value.as_str()) == Some("session.start") {
                if let Some(id) = value
                    .get("data")
                    .and_then(|value| value.get("sessionId"))
                    .and_then(|value| value.as_str())
                {
                    session_id = Some(id.to_string());
                }
            }
        }
        if let Some(ts) = value
            .get("timestamp")
            .and_then(|value| value.as_str())
            .and_then(parse_timestamp)
        {
            latest_timestamp = match latest_timestamp {
                Some(current) if current >= ts => Some(current),
                _ => Some(ts),
            };
        }
    }

    let session_id = session_id.or_else(|| {
        path.file_stem()
            .and_then(|value| value.to_str())
            .map(|value| value.to_string())
    })?;

    let timestamp = latest_timestamp;
    Some(SessionCandidate {
        session_id,
        timestamp,
    })
}

fn find_latest_session_in_dir(dir: &Path, desired_cwd: &str) -> Option<String> {
    let entries = fs::read_dir(dir).ok()?;
    let mut best: Option<SessionCandidate> = None;
    for entry in entries.flatten() {
        let path = entry.path();
        if entry.file_type().map(|ty| ty.is_file()).unwrap_or(false) {
            if let Some(candidate) = parse_session_file(&path, desired_cwd) {
                let replace = match (&candidate.timestamp, &best) {
                    (Some(candidate_ts), Some(best)) => match best.timestamp {
                        Some(best_ts) => candidate_ts > &best_ts,
                        None => true,
                    },
                    (Some(_), None) => true,
                    (None, Some(_)) => false,
                    (None, None) => true,
                };
                if replace {
                    best = Some(candidate);
                }
            }
        }
    }
    best.map(|candidate| candidate.session_id)
}

fn find_latest_session_id(worktree_path: &Path) -> anyhow::Result<Option<String>> {
    let desired_cwd = resolve_session_cwd(worktree_path)?;
    #[cfg(target_os = "windows")]
    let home_dir = resolve_wsl_home_dir(worktree_path)?;
    #[cfg(not(target_os = "windows"))]
    let home_dir = resolve_home_dir()?;
    let primary = home_dir.join(COPILOT_SESSION_DIR);
    let legacy = home_dir.join(COPILOT_LEGACY_SESSION_DIR);

    if let Some(session_id) = find_latest_session_in_dir(&primary, &desired_cwd) {
        return Ok(Some(session_id));
    }
    if let Some(session_id) = find_latest_session_in_dir(&legacy, &desired_cwd) {
        return Ok(Some(session_id));
    }

    Ok(None)
}

impl CopilotAgent {
    fn status_from_output(&self, raw: &[u8], timestamp: Instant) -> Option<TaskStatus> {
        let mut state = self.state.lock();
        state.last_output = Some(timestamp);
        state.screen.process(raw);
        let status = TaskStatus::Working;
        let status_changed = state.last_status != Some(status);
        if status_changed {
            state.last_status = Some(status);
        }
        if status_changed { Some(status) } else { None }
    }

    fn status_if_idle(&self, now: Instant) -> Option<TaskStatus> {
        let mut state = self.state.lock();
        let last = state.last_output?;
        if now.duration_since(last) >= Duration::from_millis(1000)
            && state.last_status == Some(TaskStatus::Working)
        {
            state.last_status = Some(TaskStatus::Idle);
            return Some(TaskStatus::Idle);
        }
        None
    }
}

impl Agent for CopilotAgent {
    fn start(
        &mut self,
        worktree_path: &Path,
        callbacks: AgentCallbacks,
        rows: u16,
        cols: u16,
    ) -> anyhow::Result<AgentRuntime> {
        let pty_system = native_pty_system();
        let rows = rows.max(1);
        let cols = cols.max(1);
        let maybe_session_id = find_latest_session_id(worktree_path)?;
        let mut args = vec![
            "--allow-all-tools".to_string(),
            "--deny-tool".to_string(),
            "shell(git push)".to_string(),
        ];
        if let Some(session_id) = maybe_session_id {
            args.push("--resume".to_string());
            args.push(session_id);
        }
        let pair = pty_system.openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let master = pair.master;
        let writer = master
            .take_writer()
            .context("failed to obtain pty writer")?;
        let reader = master
            .try_clone_reader()
            .context("failed to clone pty reader")?;
        let master = Arc::new(Mutex::new(master));
        let writer = Arc::new(Mutex::new(writer));

        #[cfg(target_os = "windows")]
        let command = {
            let arg_refs: Vec<&str> = args.iter().map(|arg| arg.as_str()).collect();
            build_wsl_command(worktree_path, "copilot", &arg_refs)
        };

        #[cfg(not(target_os = "windows"))]
        let command = {
            let mut command = CommandBuilder::new("copilot");
            command.args(args.iter().map(|arg| arg.as_str()));
            command.cwd(worktree_path);
            command
        };

        let child = pair
            .slave
            .spawn_command(command)
            .context("failed to start Copilot")?;
        let child: Arc<Mutex<ChildHandle>> = Arc::new(Mutex::new(child));

        let status_handle = self.clone();
        let output_callbacks = callbacks.clone();
        let running = Arc::new(AtomicBool::new(true));
        let idle_running = Arc::clone(&running);
        let idle_handle = self.clone();
        let idle_callbacks = callbacks.clone();
        std::thread::spawn(move || {
            while idle_running.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(250));
                if let Some(status) = idle_handle.status_if_idle(Instant::now()) {
                    (idle_callbacks.on_status)(status);
                }
            }
        });

        std::thread::spawn(move || {
            let mut reader = reader;
            let mut buffer = [0u8; 8192];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(size) => {
                        let now = Instant::now();
                        let chunk = String::from_utf8_lossy(&buffer[..size]).to_string();
                        if let Some(status) =
                            status_handle.status_from_output(&buffer[..size], now)
                        {
                            (output_callbacks.on_status)(status);
                        }
                        (output_callbacks.on_output)(chunk);
                    }
                    Err(_) => break,
                }
            }
        });

        let exit_callbacks = callbacks.clone();
        let exit_child = child.clone();
        let exit_running = Arc::clone(&running);
        std::thread::spawn(move || {
            let exit_code = loop {
                {
                    let mut child_guard = exit_child.lock();
                    match child_guard.try_wait() {
                        Ok(Some(status)) => {
                            let code = status.exit_code() as i32;
                            break if status.success() { 0 } else { code };
                        }
                        Ok(None) => {}
                        Err(_) => break 1,
                    }
                }
                std::thread::sleep(Duration::from_millis(200));
            };
            exit_running.store(false, Ordering::Relaxed);
            (exit_callbacks.on_exit)(exit_code);
        });

        Ok(AgentRuntime {
            child,
            writer,
            master,
        })
    }

    fn reset(&mut self, rows: usize, cols: usize) {
        let mut state = self.state.lock();
        state.screen = Screen::new(rows, cols);
        state.last_output = None;
        state.last_status = None;
    }

    fn resize(&mut self, rows: usize, cols: usize) {
        self.state.lock().screen.resize(rows, cols);
    }
}
