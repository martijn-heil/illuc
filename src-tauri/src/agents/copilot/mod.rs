use crate::agents::{Agent, AgentCallbacks, AgentRuntime, ChildHandle};
use crate::tasks::TaskStatus;
use crate::utils::screen::Screen;
#[cfg(target_os = "windows")]
use crate::utils::windows::build_wsl_command;
use anyhow::Context;
use parking_lot::Mutex;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const DEFAULT_ROWS: u16 = 40;
const DEFAULT_COLS: u16 = 80;

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
        let mut command = build_wsl_command(worktree_path, "copilot", &["--allow-all-tools", "--deny-tool", "shell(git push)"]);

        #[cfg(not(target_os = "windows"))]
        let command = {
            let mut command = CommandBuilder::new("copilot");
            command.args(["--allow-all-tools", "--deny-tool", "shell(git push)"]);
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
