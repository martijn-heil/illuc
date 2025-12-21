use anyhow::Context;
use chrono::{DateTime, Utc};
use parking_lot::{Mutex, RwLock};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};
use thiserror::Error;
use uuid::Uuid;
use vte::Parser;

type Result<T> = std::result::Result<T, TaskError>;
type ChildHandle = Box<dyn Child + Send + Sync>;

#[derive(Debug, Clone)]
struct WorktreeEntry {
    path: PathBuf,
    head: String,
    branch: Option<String>,
}

#[derive(Debug, Error)]
pub enum TaskError {
    #[error("{0}")]
    Message(String),
    #[error("git command failed: {command}")]
    GitCommand { command: String, stderr: String },
    #[error("task not found")]
    NotFound,
    #[error("task is already running")]
    AlreadyRunning,
    #[error("task is not running")]
    NotRunning,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TaskStatus {
    CreatingWorktree,
    Ready,
    Idle,
    AwaitingApproval,
    Working,
    Completed,
    Failed,
    Stopped,
    Discarded,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskSummary {
    pub task_id: Uuid,
    pub title: String,
    pub status: TaskStatus,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub ended_at: Option<DateTime<Utc>>,
    pub worktree_path: String,
    pub branch_name: String,
    pub base_branch: String,
    pub base_repo_path: String,
    pub base_commit: String,
    pub exit_code: Option<i32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BaseRepoInfo {
    pub path: String,
    pub canonical_path: String,
    pub current_branch: String,
    pub head: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTaskRequest {
    pub base_repo_path: String,
    pub task_title: Option<String>,
    pub base_ref: Option<String>,
    pub branch_name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartTaskRequest {
    pub task_id: Uuid,
    pub codex_args: Option<Vec<String>>,
    pub env: Option<HashMap<String, String>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StopTaskRequest {
    pub task_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscardTaskRequest {
    pub task_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalWriteRequest {
    pub task_id: Uuid,
    pub data: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalResizeRequest {
    pub task_id: Uuid,
    pub cols: u16,
    pub rows: u16,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffRequest {
    pub task_id: Uuid,
    pub ignore_whitespace: Option<bool>,
    pub mode: Option<DiffMode>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskActionRequest {
    pub task_id: Uuid,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffPayload {
    pub task_id: Uuid,
    pub files: Vec<DiffFile>,
    pub unified_diff: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DiffMode {
    Worktree,
    Branch,
}

impl Default for DiffMode {
    fn default() -> Self {
        DiffMode::Worktree
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffFile {
    pub path: String,
    pub status: String,
}

struct TaskRecord {
    last_output: Option<Instant>,
    screen: Screen,
    parser: Parser,
    summary: TaskSummary,
    runtime: Option<TaskRuntime>,
    terminal_buffer: String,
}

struct TaskRuntime {
    child: Arc<Mutex<ChildHandle>>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
}

#[derive(Debug, Clone)]
struct Screen {
    rows: usize,
    cols: usize,
    grid: Vec<Vec<char>>,
    cursor_row: usize,
    cursor_col: usize,
}

impl Screen {
    fn new(rows: usize, cols: usize) -> Self {
        Self {
            rows,
            cols,
            grid: vec![vec![' '; cols]; rows],
            cursor_row: 0,
            cursor_col: 0,
        }
    }

    fn resize(&mut self, rows: usize, cols: usize) {
        let mut new_grid = vec![vec![' '; cols]; rows];
        let min_rows = rows.min(self.rows);
        let min_cols = cols.min(self.cols);
        for r in 0..min_rows {
            for c in 0..min_cols {
                new_grid[r][c] = *self.grid.get(r).and_then(|row| row.get(c)).unwrap_or(&' ');
            }
        }
        self.rows = rows;
        self.cols = cols;
        self.grid = new_grid;
        self.cursor_row = self.cursor_row.min(self.rows.saturating_sub(1));
        self.cursor_col = self.cursor_col.min(self.cols.saturating_sub(1));
    }

    fn scroll_up(&mut self, lines: usize) {
        for _ in 0..lines {
            self.grid.remove(0);
            self.grid.push(vec![' '; self.cols]);
        }
        self.cursor_row = self.cursor_row.saturating_sub(lines);
    }

    fn clear_screen(&mut self) {
        for row in &mut self.grid {
            for cell in row {
                *cell = ' ';
            }
        }
        self.cursor_row = 0;
        self.cursor_col = 0;
    }

    fn clear_line_from_cursor(&mut self) {
        if self.cursor_row < self.rows {
            for c in self.cursor_col..self.cols {
                self.grid[self.cursor_row][c] = ' ';
            }
        }
    }

    fn set_cursor(&mut self, row: usize, col: usize) {
        self.cursor_row = row.min(self.rows.saturating_sub(1));
        self.cursor_col = col.min(self.cols.saturating_sub(1));
    }

    fn full_text(&self) -> String {
        self.grid
            .iter()
            .map(|row| {
                let mut s: String = row.iter().collect();
                while s.ends_with(' ') {
                    s.pop();
                }
                s
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

struct ScreenPerformer<'a> {
    screen: &'a mut Screen,
}

impl<'a> ScreenPerformer<'a> {
    fn new(screen: &'a mut Screen) -> Self {
        Self { screen }
    }
}

impl<'a> vte::Perform for ScreenPerformer<'a> {
    fn print(&mut self, c: char) {
        if self.screen.cursor_row >= self.screen.rows {
            self.screen.scroll_up(1);
            self.screen.cursor_row = self.screen.rows.saturating_sub(1);
        }
        if self.screen.cursor_col >= self.screen.cols {
            self.screen.cursor_col = 0;
            self.screen.cursor_row += 1;
            if self.screen.cursor_row >= self.screen.rows {
                self.screen.scroll_up(1);
                self.screen.cursor_row = self.screen.rows.saturating_sub(1);
            }
        }
        if self.screen.cursor_row < self.screen.rows && self.screen.cursor_col < self.screen.cols {
            self.screen.grid[self.screen.cursor_row][self.screen.cursor_col] = c;
            self.screen.cursor_col += 1;
        }
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            b'\n' => {
                self.screen.cursor_col = 0;
                self.screen.cursor_row += 1;
                if self.screen.cursor_row >= self.screen.rows {
                    self.screen.scroll_up(1);
                    self.screen.cursor_row = self.screen.rows.saturating_sub(1);
                }
            }
            b'\r' => self.screen.cursor_col = 0,
            b'\x08' => {
                if self.screen.cursor_col > 0 {
                    self.screen.cursor_col -= 1;
                }
            }
            b'\t' => {
                let next_tab = ((self.screen.cursor_col / 8) + 1) * 8;
                self.screen.cursor_col = next_tab.min(self.screen.cols.saturating_sub(1));
            }
            _ => {}
        }
    }

    fn csi_dispatch(
        &mut self,
        params: &vte::Params,
        _intermediates: &[u8],
        _ignore: bool,
        action: char,
    ) {
        let first_param = |idx: usize, default: usize| -> usize {
            params
                .iter()
                .nth(idx)
                .and_then(|p| p.get(0))
                .map(|v| (*v).max(1) as usize)
                .unwrap_or(default)
        };

        match action {
            'A' => {
                let n = first_param(0, 1);
                self.screen.cursor_row = self.screen.cursor_row.saturating_sub(n);
            }
            'B' => {
                let n = first_param(0, 1);
                self.screen.cursor_row = (self.screen.cursor_row + n).min(self.screen.rows - 1);
            }
            'C' => {
                let n = first_param(0, 1);
                self.screen.cursor_col = (self.screen.cursor_col + n).min(self.screen.cols - 1);
            }
            'D' => {
                let n = first_param(0, 1);
                self.screen.cursor_col = self.screen.cursor_col.saturating_sub(n);
            }
            'H' | 'f' => {
                let row = first_param(0, 1);
                let col = first_param(1, 1);
                self.screen.set_cursor(row.saturating_sub(1), col.saturating_sub(1));
            }
            'J' => {
                let mode = first_param(0, 0);
                if mode == 2 {
                    self.screen.clear_screen();
                } else {
                    self.screen.clear_line_from_cursor();
                }
            }
            'K' => self.screen.clear_line_from_cursor(),
            _ => {}
        }
    }

    fn hook(
        &mut self,
        _params: &vte::Params,
        _intermediates: &[u8],
        _ignore: bool,
        _action: char,
    ) {
    }

    fn put(&mut self, _byte: u8) {}

    fn unhook(&mut self) {}
}

#[derive(Clone, Default)]
pub struct TaskManager {
    inner: Arc<TaskManagerInner>,
}

#[derive(Default)]
struct TaskManagerInner {
    tasks: RwLock<HashMap<Uuid, TaskRecord>>,
}

impl TaskManager {
    pub fn create_task(
        &self,
        req: CreateTaskRequest,
        app: &AppHandle,
    ) -> Result<TaskSummary> {
        let CreateTaskRequest {
            base_repo_path,
            task_title,
            base_ref,
            branch_name,
        } = req;

        let base_repo = PathBuf::from(base_repo_path);
        let repo_root = get_repo_root(&base_repo)?;
        ensure_directory(&base_repo)?;

        validate_git_repo(&base_repo)?;

        let base_ref = base_ref.unwrap_or_else(|| "HEAD".to_string());
        let base_commit = run_git(&repo_root, ["rev-parse", base_ref.as_str()])?;

        let task_id = Uuid::new_v4();
        let title = task_title.unwrap_or_else(|| format!("Task {}", task_id.simple()));
        let timestamp = Utc::now();
        let branch_name = branch_name
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| TaskError::Message("Branch name is required.".into()))?;

        let managed_root = managed_worktree_root(&repo_root)?;
        let worktree_path = managed_root.join(task_id.to_string());

        if worktree_path.exists() {
            std::fs::remove_dir_all(&worktree_path).ok();
        }

        let worktree_path_str = worktree_path.to_string_lossy().to_string();
        run_git(
            &repo_root,
            [
                "worktree",
                "add",
                "-b",
                branch_name.as_str(),
                worktree_path_str.as_str(),
                base_ref.as_str(),
            ],
        )?;

        let summary = TaskSummary {
            task_id,
            title,
            status: TaskStatus::Ready,
            created_at: timestamp,
            started_at: None,
            ended_at: None,
            worktree_path: worktree_path_str,
            branch_name,
            base_branch: base_ref.clone(),
            base_repo_path: repo_root.to_string_lossy().to_string(),
            base_commit,
            exit_code: None,
        };

        let mut tasks = self.inner.tasks.write();
        tasks.insert(
            task_id,
            TaskRecord {
                last_output: None,
                screen: Screen::new(40, 120),
                parser: Parser::new(),
                summary: summary.clone(),
                runtime: None,
                terminal_buffer: String::new(),
            },
        );
        drop(tasks);
        emit_status(app, &summary);
        Ok(summary)
    }

    pub fn start_task(
        &self,
        req: StartTaskRequest,
        app: &AppHandle,
    ) -> Result<TaskSummary> {
        let StartTaskRequest {
            task_id,
            codex_args,
            env,
        } = req;
        {
            let tasks = self.inner.tasks.read();
            let record = tasks.get(&task_id).ok_or(TaskError::NotFound)?;
            if record.runtime.is_some() {
                return Err(TaskError::AlreadyRunning);
            }
        }

        let (worktree_path, title, _has_started) = {
            let tasks = self.inner.tasks.read();
            let record = tasks.get(&task_id).ok_or(TaskError::NotFound)?;
            (
                PathBuf::from(&record.summary.worktree_path),
                record.summary.title.clone(),
                record.summary.started_at.is_some(),
            )
        };

        let pty_system = native_pty_system();
        let pair = pty_system.openpty(PtySize {
            rows: 40,
            cols: 120,
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

        let args = if let Some(explicit) = codex_args {
            explicit
        } else {
            vec!["resume".to_string()]
        };

        let mut command = CommandBuilder::new("codex");
        command.args(args.iter().map(|s| s.as_str()));
        command.cwd(&worktree_path);
        if let Some(env) = env {
            for (key, value) in env {
                command.env(key, value);
            }
        }

        let child = pair
            .slave
            .spawn_command(command)
            .with_context(|| format!("failed to start Codex for task {}", title))?;
        let child: Arc<Mutex<ChildHandle>> = Arc::new(Mutex::new(child));

        {
            let mut tasks = self.inner.tasks.write();
            let record = tasks
                .get_mut(&task_id)
                .ok_or(TaskError::NotFound)?;
            record.summary.status = TaskStatus::Idle;
            record.summary.started_at = Some(Utc::now());
            record.summary.exit_code = None;
            record.runtime = Some(TaskRuntime {
                child: child.clone(),
                writer: writer.clone(),
                master: master.clone(),
            });
            record.screen = Screen::new(40, 120);
            record.parser = Parser::new();
            emit_status(app, &record.summary);
        }

        let reader_manager = self.clone();
        let reader_app = app.clone();
        std::thread::spawn(move || {
            stream_terminal_output(reader, reader_manager, reader_app, task_id);
        });

        let exit_manager = self.clone();
        let exit_app = app.clone();
        tauri::async_runtime::spawn(async move {
            wait_for_exit(exit_manager, exit_app, task_id, child).await;
        });

        let tasks = self.inner.tasks.read();
        let record = tasks.get(&task_id).ok_or(TaskError::NotFound)?;
        Ok(record.summary.clone())
    }

    pub fn stop_task(
        &self,
        req: StopTaskRequest,
        app: &AppHandle,
    ) -> Result<TaskSummary> {
        let task_id = req.task_id;
        let child = {
            let tasks = self.inner.tasks.read();
            let record = tasks.get(&task_id).ok_or(TaskError::NotFound)?;
            if let Some(runtime) = &record.runtime {
                runtime.child.clone()
            } else {
                return Err(TaskError::NotRunning);
            }
        };

        if let Some(mut child_guard) = child.try_lock() {
            let _ = child_guard.kill();
        }

        {
            let mut tasks = self.inner.tasks.write();
            let record = tasks
                .get_mut(&task_id)
                .ok_or(TaskError::NotFound)?;
            record.summary.status = TaskStatus::Ready;
            emit_status(app, &record.summary);
            return Ok(record.summary.clone());
        }
    }

    pub fn discard_task(&self, req: DiscardTaskRequest, app: &AppHandle) -> Result<()> {
        let task_id = req.task_id;
        let (worktree_path, branch_name, base_repo_path, runtime_exists) = {
            let tasks = self.inner.tasks.read();
            let record = tasks.get(&task_id).ok_or(TaskError::NotFound)?;
            (
                PathBuf::from(&record.summary.worktree_path),
                record.summary.branch_name.clone(),
                PathBuf::from(&record.summary.base_repo_path),
                record.runtime.is_some(),
            )
        };

        if runtime_exists {
            let _ = self.stop_task(StopTaskRequest { task_id }, app);
        }

        let worktree_path_string = worktree_path.to_string_lossy().to_string();
        let _ = run_git(
            &base_repo_path,
            [
                "worktree",
                "remove",
                "--force",
                worktree_path_string.as_str(),
            ],
        );
        let _ = run_git(&base_repo_path, ["branch", "-D", branch_name.as_str()]);
        if worktree_path.exists() {
            let _ = std::fs::remove_dir_all(&worktree_path);
        }

        {
            let mut tasks = self.inner.tasks.write();
            if let Some(record) = tasks.get_mut(&task_id) {
                record.summary.status = TaskStatus::Discarded;
                record.runtime = None;
                emit_status(app, &record.summary);
            }
        }

        let mut tasks = self.inner.tasks.write();
        tasks.remove(&task_id);
        Ok(())
    }

    pub fn terminal_write(&self, req: TerminalWriteRequest) -> Result<()> {
        let task_id = req.task_id;
        let writer = {
            let tasks = self.inner.tasks.read();
            let record = tasks.get(&task_id).ok_or(TaskError::NotFound)?;
            match &record.runtime {
                Some(runtime) => runtime.writer.clone(),
                None => return Err(TaskError::NotRunning),
            }
        };
        let mut writer_guard = writer.lock();
        writer_guard
            .write_all(req.data.as_bytes())
            .context("failed to write to terminal")?;
        writer_guard.flush().ok();
        Ok(())
    }

    pub fn terminal_resize(&self, req: TerminalResizeRequest) -> Result<()> {
        let task_id = req.task_id;
        let master = {
            let tasks = self.inner.tasks.read();
            let record = tasks.get(&task_id).ok_or(TaskError::NotFound)?;
            match &record.runtime {
                Some(runtime) => runtime.master.clone(),
                None => return Err(TaskError::NotRunning),
            }
        };
        master
            .lock()
            .resize(PtySize {
                cols: req.cols,
                rows: req.rows,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("failed to resize terminal")?;
        {
            let mut tasks = self.inner.tasks.write();
            if let Some(record) = tasks.get_mut(&task_id) {
                record.screen.resize(req.rows as usize, req.cols as usize);
            }
        }
        Ok(())
    }

    pub fn get_diff(&self, req: DiffRequest) -> Result<DiffPayload> {
        let task_id = req.task_id;
        let (worktree_path, base_commit) = {
            let tasks = self.inner.tasks.read();
            let record = tasks.get(&task_id).ok_or(TaskError::NotFound)?;
            (
                PathBuf::from(&record.summary.worktree_path),
                record.summary.base_commit.clone(),
            )
        };

        let whitespace_flag = if req.ignore_whitespace.unwrap_or(false) {
            Some("--ignore-all-space")
        } else {
            None
        };
        let mode = req.mode.unwrap_or(DiffMode::Worktree);
        match mode {
            DiffMode::Worktree => {
                let staged = git_diff(
                    worktree_path.as_path(),
                    Some("--cached"),
                    "HEAD",
                    whitespace_flag,
                )?;
                let unstaged =
                    git_diff(worktree_path.as_path(), None, "HEAD", whitespace_flag)?;

                let diff_output = format!("{}\n{}", staged.diff, unstaged.diff)
                    .trim()
                    .to_string();
                let files = merge_diff_files(staged.files, unstaged.files);

                Ok(DiffPayload {
                    task_id,
                    files,
                    unified_diff: diff_output,
                })
            }
            DiffMode::Branch => {
                let branch_diff = git_diff_branch(
                    worktree_path.as_path(),
                    base_commit.as_str(),
                    whitespace_flag,
                )?;
                Ok(DiffPayload {
                    task_id,
                    files: branch_diff.files,
                    unified_diff: branch_diff.diff,
                })
            }
        }
    }

    fn append_terminal_output(&self, task_id: Uuid, chunk: &str, raw: &[u8], timestamp: Instant) {
        if let Some(record) = self.inner.tasks.write().get_mut(&task_id) {
            record.terminal_buffer.push_str(chunk);
            record.last_output = Some(timestamp);

            let mut performer = ScreenPerformer::new(&mut record.screen);
            for byte in raw {
                record.parser.advance(&mut performer, *byte);
            }
        }
    }

    fn mark_waiting_for_approval(&self, task_id: Uuid, app: &AppHandle) {
        let mut tasks = self.inner.tasks.write();
        if let Some(record) = tasks.get_mut(&task_id) {
            if record.summary.status != TaskStatus::AwaitingApproval {
                record.summary.status = TaskStatus::AwaitingApproval;
                emit_status(app, &record.summary);
            }
        }
    }

    fn has_recent_approval_prompt(&self, task_id: Uuid) -> bool {
        const PROMPT: &str = "would you like to run the following command";
        let tasks = self.inner.tasks.read();
        tasks
            .get(&task_id)
            .map(|record| {
                record
                    .screen
                    .full_text()
                    .to_ascii_lowercase()
                    .contains(PROMPT)
            })
            .unwrap_or(false)
    }

    fn mark_idle_from_awaiting(&self, task_id: Uuid, app: &AppHandle) {
        let mut tasks = self.inner.tasks.write();
        if let Some(record) = tasks.get_mut(&task_id) {
            if record.runtime.is_some() && record.summary.status == TaskStatus::AwaitingApproval {
                record.summary.status = TaskStatus::Idle;
                emit_status(app, &record.summary);
            }
        }
    }

    fn mark_working_if_needed(&self, task_id: Uuid, app: &AppHandle) {
        let mut tasks = self.inner.tasks.write();
        if let Some(record) = tasks.get_mut(&task_id) {
            if record.summary.status != TaskStatus::Working {
                record.summary.status = TaskStatus::Working;
                emit_status(app, &record.summary);
            }
        }
    }

    fn mark_idle_if_quiet(&self, task_id: Uuid, timestamp: Instant, app: &AppHandle) {
        let mut tasks = self.inner.tasks.write();
        if let Some(record) = tasks.get_mut(&task_id) {
            if let Some(last) = record.last_output {
                if last <= timestamp
                    && record.runtime.is_some()
                    && record.summary.status == TaskStatus::Working
                {
                    record.summary.status = TaskStatus::Idle;
                    emit_status(app, &record.summary);
                }
            }
        }
    }

    fn contains_worktree_path(&self, path: &Path) -> bool {
        let target = path.to_string_lossy();
        self.inner
            .tasks
            .read()
            .values()
            .any(|record| record.summary.worktree_path == target)
    }

    pub fn register_existing_worktrees(
        &self,
        base_repo_path: String,
        app: &AppHandle,
    ) -> Result<Vec<TaskSummary>> {
        let provided_path = PathBuf::from(&base_repo_path);
        ensure_directory(&provided_path)?;
        validate_git_repo(&provided_path)?;
        let repo_root = get_repo_root(&provided_path)?
            .canonicalize()
            .unwrap_or_else(|_| provided_path.clone());
        let managed_root = managed_worktree_root(&repo_root)?;
        let base_repo_head = run_git(&repo_root, ["rev-parse", "HEAD"])?;
        let base_repo_branch =
            run_git(&repo_root, ["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_else(|_| {
                "HEAD".to_string()
            });
        let entries = list_worktrees(&repo_root)?;
        let mut inserted = Vec::new();
        for entry in entries {
            let canonical_path = entry
                .path
                .canonicalize()
                .unwrap_or_else(|_| entry.path.clone());
            if canonical_path == repo_root {
                continue;
            }
            if !canonical_path.starts_with(&managed_root) {
                continue;
            }
            if self.contains_worktree_path(&canonical_path) {
                continue;
            }
            let branch_name = entry
                .branch
                .as_ref()
                .map(|name| clean_branch_name(name))
                .unwrap_or_else(|| {
                    let short_head: String = entry.head.chars().take(7).collect();
                    format!("detached-{}", short_head)
                });
            let summary = TaskSummary {
                task_id: Uuid::new_v4(),
                title: format_title_from_branch(&branch_name),
                status: TaskStatus::Ready,
                created_at: Utc::now(),
                started_at: None,
                ended_at: None,
                worktree_path: canonical_path.to_string_lossy().to_string(),
                branch_name,
                base_branch: base_repo_branch.clone(),
                base_repo_path: repo_root.to_string_lossy().to_string(),
                base_commit: base_repo_head.clone(),
                exit_code: None,
            };
            self.inner.tasks.write().insert(
                summary.task_id,
                TaskRecord {
                    last_output: None,
                    screen: Screen::new(40, 120),
                    parser: Parser::new(),
                    summary: summary.clone(),
                    runtime: None,
                    terminal_buffer: String::new(),
                },
            );
            emit_status(app, &summary);
            inserted.push(summary);
        }
        Ok(inserted)
    }

    fn worktree_path(&self, task_id: Uuid) -> Result<PathBuf> {
        let tasks = self.inner.tasks.read();
        let record = tasks.get(&task_id).ok_or(TaskError::NotFound)?;
        Ok(PathBuf::from(&record.summary.worktree_path))
    }

    pub fn open_in_vscode(&self, req: TaskActionRequest) -> Result<()> {
        let path = self.worktree_path(req.task_id)?;
        spawn_vscode(&path)
    }

    pub fn open_terminal(&self, req: TaskActionRequest) -> Result<()> {
        let path = self.worktree_path(req.task_id)?;
        spawn_terminal(&path)
    }

    fn finish_task(&self, task_id: Uuid, exit_code: i32, app: &AppHandle) -> Result<()> {
        let mut tasks = self.inner.tasks.write();
        let record = tasks
            .get_mut(&task_id)
            .ok_or(TaskError::NotFound)?;
        record.summary.exit_code = Some(exit_code);
        record.summary.ended_at = Some(Utc::now());
        record.runtime = None;
        let target_status = match record.summary.status {
            TaskStatus::Stopped => TaskStatus::Stopped,
            TaskStatus::Discarded => TaskStatus::Discarded,
            TaskStatus::Ready => TaskStatus::Ready,
            _ if exit_code == 0 => TaskStatus::Completed,
            _ => TaskStatus::Failed,
        };
        record.summary.status = target_status;
        emit_status(app, &record.summary);
        Ok(())
    }
}

fn stream_terminal_output(
    mut reader: Box<dyn Read + Send>,
    manager: TaskManager,
    app: AppHandle,
    task_id: Uuid,
) {
    let mut buffer = [0u8; 8192];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(size) => {
                let now = Instant::now();
                let chunk = String::from_utf8_lossy(&buffer[..size]).to_string();
                manager.append_terminal_output(task_id, &chunk, &buffer[..size], now);
                if manager.has_recent_approval_prompt(task_id) {
                    manager.mark_waiting_for_approval(task_id, &app);
                } else {
                    manager.mark_working_if_needed(task_id, &app);
                    manager.mark_idle_from_awaiting(task_id, &app);
                    let mgr = manager.clone();
                    let app_clone = app.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_millis(1000));
                        mgr.mark_idle_if_quiet(task_id, now, &app_clone);
                    });
                }
                let payload = TerminalOutputPayload {
                    task_id,
                    data: chunk.clone(),
                };
                let _ = app.emit("task_terminal_output", payload);
            }
            Err(_) => break,
        }
    }
}

async fn wait_for_exit(
    manager: TaskManager,
    app: AppHandle,
    task_id: Uuid,
    child: Arc<Mutex<ChildHandle>>,
) {
    let exit_code = tauri::async_runtime::spawn_blocking(move || loop {
        {
            let mut child_guard = child.lock();
            match child_guard.try_wait() {
                Ok(Some(status)) => {
                    let code = status.exit_code() as i32;
                    return if status.success() { 0 } else { code };
                }
                Ok(None) => {}
                Err(_) => return 1,
            }
        }
        std::thread::sleep(Duration::from_millis(200));
    })
    .await
    .unwrap_or(1);

    let _ = manager.finish_task(task_id, exit_code, &app);
    let payload = TerminalExitPayload {
        task_id,
        exit_code,
    };
    let _ = app.emit("task_terminal_exit", payload);
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct TerminalOutputPayload {
    task_id: Uuid,
    data: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct TerminalExitPayload {
    task_id: Uuid,
    exit_code: i32,
}

fn ensure_directory(path: &Path) -> Result<()> {
    if path.exists() {
        if path.is_dir() {
            Ok(())
        } else {
            Err(TaskError::Message(format!(
                "{} is not a directory",
                path.display()
            )))
        }
    } else {
        Err(TaskError::Message(format!(
            "{} does not exist",
            path.display()
        )))
    }
}

fn validate_git_repo(path: &Path) -> Result<()> {
    run_git(path, ["rev-parse", "--show-toplevel"]).map(|_| ())
}

fn get_repo_root(path: &Path) -> Result<PathBuf> {
    let root = run_git(path, ["rev-parse", "--show-toplevel"])?;
    Ok(PathBuf::from(root))
}

fn managed_worktree_root(repo_root: &Path) -> Result<PathBuf> {
    let illuc_dir = repo_root.join(".illuc");
    let worktree_dir = illuc_dir.join("worktrees");
    if !worktree_dir.exists() {
        std::fs::create_dir_all(&worktree_dir)?;
    }
    Ok(worktree_dir)
}

pub fn open_path_in_vscode(path: &str) -> Result<()> {
    let target = PathBuf::from(path);
    ensure_directory(&target)?;
    spawn_vscode(&target)
}

pub fn open_path_terminal(path: &str) -> Result<()> {
    let target = PathBuf::from(path);
    ensure_directory(&target)?;
    spawn_terminal(&target)
}

pub fn list_branches(path: String) -> Result<Vec<String>> {
    let repo = PathBuf::from(&path);
    ensure_directory(&repo)?;
    validate_git_repo(&repo)?;
    let output = run_git(&repo, ["branch", "--format=%(refname:short)"])?;
    let branches = output
        .lines()
        .map(|line| line.trim().trim_start_matches('*').trim().to_string())
        .filter(|line| !line.is_empty())
        .collect();
    Ok(branches)
}

fn spawn_vscode(path: &Path) -> Result<()> {
    #[cfg(windows)]
    let candidates = ["code.cmd", "code.exe", "code"];
    #[cfg(not(windows))]
    let candidates = ["code"];

    for candidate in candidates {
        let result = Command::new(candidate).arg(path).spawn();
        match result {
            Ok(_) => return Ok(()),
            Err(err) => {
                if err.kind() == std::io::ErrorKind::NotFound {
                    continue;
                } else {
                    return Err(err.into());
                }
            }
        }
    }
    Err(TaskError::Message(
        "Unable to launch VS Code. Make sure the `code` command is available.".to_string(),
    ))
}

fn spawn_terminal(path: &Path) -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        let path_str = path.to_string_lossy().to_string();
        let mut attempt_cmd = |mut command: Command| -> Result<bool> {
            match command.spawn() {
                Ok(_) => Ok(true),
                Err(err) => {
                    if err.kind() == std::io::ErrorKind::NotFound {
                        Ok(false)
                    } else {
                        Err(err.into())
                    }
                }
            }
        };

        if attempt_cmd({
            let mut cmd = Command::new("wt");
            cmd.args(["-d", &path_str]);
            cmd
        })? {
            return Ok(());
        }

        for candidate in ["alacritty", "alacritty.exe"] {
            if attempt_cmd({
                let mut cmd = Command::new(candidate);
                cmd.args(["--working-directory", &path_str]);
                cmd
            })? {
                return Ok(());
            }
        }

        if attempt_cmd({
            let mut cmd = Command::new("cmd");
            cmd.args([
                "/C",
                "start",
                "cmd",
                "/K",
                &format!("cd /d \"{}\"", path_str),
            ]);
            cmd
        })? {
            return Ok(());
        }

        if attempt_cmd({
            let mut cmd = Command::new("cmd");
            cmd.args([
                "/C",
                "start",
                "powershell",
                "-NoExit",
                "-Command",
                &format!("Set-Location -Path \"{}\"", path_str),
            ]);
            cmd
        })? {
            return Ok(());
        }

        Err(TaskError::Message(
            "Unable to launch a terminal window. Install Windows Terminal or ensure cmd.exe is available."
                .to_string(),
        ))
    }
    #[cfg(not(target_os = "windows"))]
    {
        let path_str = path.to_string_lossy().to_string();
        let attempts: Vec<(&str, Vec<&str>)> = vec![
            (
                "x-terminal-emulator",
                vec!["--working-directory", path_str.as_str()],
            ),
            (
                "gnome-terminal",
                vec!["--working-directory", path_str.as_str()],
            ),
            ("konsole", vec!["--workdir", path_str.as_str()]),
            (
                "xfce4-terminal",
                vec!["--working-directory", path_str.as_str()],
            ),
            ("kitty", vec!["--directory", path_str.as_str()]),
            ("alacritty", vec!["--working-directory", path_str.as_str()]),
            ("terminator", vec!["--working-directory", path_str.as_str()]),
            ("tilix", vec!["--working-directory", path_str.as_str()]),
        ];
        for (bin, args) in attempts {
            let result = Command::new(bin).args(args).spawn();
            match result {
                Ok(_) => return Ok(()),
                Err(err) => {
                    if err.kind() == std::io::ErrorKind::NotFound {
                        continue;
                    } else {
                        return Err(err.into());
                    }
                }
            }
        }
        Err(TaskError::Message(
            "Unable to find a supported terminal application. Install gnome-terminal, kitty, or another supported terminal."
                .to_string(),
        ))
    }
}

fn list_worktrees(repo: &Path) -> Result<Vec<WorktreeEntry>> {
    let output = run_git(repo, ["worktree", "list", "--porcelain"])?;
    let mut entries = Vec::new();
    let mut current: Option<WorktreeEntry> = None;
    for line in output.lines() {
        if line.trim().is_empty() {
            if let Some(entry) = current.take() {
                entries.push(entry);
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("worktree ") {
            if let Some(entry) = current.take() {
                entries.push(entry);
            }
            current = Some(WorktreeEntry {
                path: PathBuf::from(rest.trim()),
                head: String::new(),
                branch: None,
            });
        } else if let Some(rest) = line.strip_prefix("HEAD ") {
            if let Some(entry) = current.as_mut() {
                entry.head = rest.trim().to_string();
            }
        } else if let Some(rest) = line.strip_prefix("branch ") {
            if let Some(entry) = current.as_mut() {
                entry.branch = Some(rest.trim().to_string());
            }
        }
    }
    if let Some(entry) = current.take() {
        entries.push(entry);
    }
    Ok(entries)
}

fn clean_branch_name(branch: &str) -> String {
    branch
        .trim()
        .strip_prefix("refs/heads/")
        .unwrap_or(branch.trim())
        .to_string()
}

fn format_title_from_branch(branch: &str) -> String {
    let slug = branch.split('/').last().unwrap_or(branch);
    let (task_id, label) = extract_task_and_label(slug);
    if let Some(task) = task_id {
        format!("[{}] {}", task, label)
    } else {
        label
    }
}

fn extract_task_and_label(slug: &str) -> (Option<String>, String) {
    let mut range: Option<(usize, usize)> = None;
    let mut digits = String::new();
    let mut iter = slug.char_indices().peekable();
    while let Some((start_idx, ch)) = iter.next() {
        if ch.is_ascii_digit() {
            digits.clear();
            digits.push(ch);
            let mut end_idx = start_idx + ch.len_utf8();
            while let Some(&(next_idx, next_ch)) = iter.peek() {
                if next_ch.is_ascii_digit() {
                    digits.push(next_ch);
                    end_idx = next_idx + next_ch.len_utf8();
                    iter.next();
                } else {
                    break;
                }
            }
            if digits.len() >= 3 {
                range = Some((start_idx, end_idx));
                break;
            }
        }
    }

    let mut remainder = slug.to_string();
    let task_id = if let Some((start, end)) = range {
        let task = remainder[start..end].to_string();
        remainder.replace_range(start..end, " ");
        Some(task)
    } else {
        None
    };

    let cleaned = remainder
        .replace(&['-', '_'][..], " ")
        .split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => {
                    first.to_uppercase().collect::<String>()
                        + chars.as_str().to_lowercase().as_str()
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ");

    let label = if cleaned.is_empty() {
        slug.replace(&['/', '-', '_'][..], " ")
            .split_whitespace()
            .map(|word| {
                let mut chars = word.chars();
                match chars.next() {
                    Some(first) => {
                        first.to_uppercase().collect::<String>()
                            + chars.as_str().to_lowercase().as_str()
                    }
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        cleaned
    };

    (task_id, label.trim().to_string())
}

struct DiffResult {
    diff: String,
    files: Vec<DiffFile>,
}

fn git_diff(
    repo: &Path,
    mode: Option<&str>,
    base_commit: &str,
    whitespace_flag: Option<&str>,
) -> Result<DiffResult> {
    let mut diff_args = vec!["diff".to_string()];
    if let Some(flag) = whitespace_flag {
        diff_args.push(flag.to_string());
    }
    if let Some(mode_flag) = mode {
        diff_args.push(mode_flag.to_string());
    }
    diff_args.push(base_commit.to_string());
    let diff_output = run_git(repo, diff_args.iter().map(String::as_str))?;

    let mut files_args = vec!["diff".to_string(), "--name-status".to_string()];
    if let Some(flag) = whitespace_flag {
        files_args.insert(1, flag.to_string());
    }
    if let Some(mode_flag) = mode {
        files_args.push(mode_flag.to_string());
    }
    files_args.push(base_commit.to_string());
    let files_output = run_git(repo, files_args.iter().map(String::as_str))?;
    let files = parse_diff_files(&files_output);

    Ok(DiffResult {
        diff: if mode == Some("--cached") {
            format!("--- Staged Changes ---\n{}", diff_output)
        } else {
            format!("--- Unstaged Changes ---\n{}", diff_output)
        },
        files,
    })
}

fn git_diff_branch(
    repo: &Path,
    base_commit: &str,
    whitespace_flag: Option<&str>,
) -> Result<DiffResult> {
    let mut diff_args = vec!["diff".to_string()];
    if let Some(flag) = whitespace_flag {
        diff_args.push(flag.to_string());
    }
    diff_args.push(base_commit.to_string());
    let diff_output = run_git(repo, diff_args.iter().map(String::as_str))?;

    let mut files_args = vec!["diff".to_string(), "--name-status".to_string()];
    if let Some(flag) = whitespace_flag {
        files_args.insert(1, flag.to_string());
    }
    files_args.push(base_commit.to_string());
    let files_output = run_git(repo, files_args.iter().map(String::as_str))?;
    let files = parse_diff_files(&files_output);
    let short_base = &base_commit[..std::cmp::min(7, base_commit.len())];
    Ok(DiffResult {
        diff: format!(
            "--- Branch comparison vs {} ---\n{}",
            short_base, diff_output
        ),
        files,
    })
}

fn merge_diff_files(mut staged: Vec<DiffFile>, mut unstaged: Vec<DiffFile>) -> Vec<DiffFile> {
    staged.append(&mut unstaged);
    let mut combined = Vec::new();
    for file in staged {
        if !combined
            .iter()
            .any(|existing: &DiffFile| existing.path == file.path)
        {
            combined.push(file);
        }
    }
    combined
}

fn run_git<I, S>(repo: &Path, args: I) -> Result<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args_vec: Vec<OsString> = args
        .into_iter()
        .map(|a| a.as_ref().to_os_string())
        .collect();
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(&args_vec)
        .output()?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(TaskError::GitCommand {
            command: format!(
                "git -C {} {}",
                repo.display(),
                args_vec
                    .iter()
                    .map(|arg| arg.to_string_lossy().to_string())
                    .collect::<Vec<_>>()
                    .join(" ")
            ),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        })
    }
}

fn parse_diff_files(output: &str) -> Vec<DiffFile> {
    output
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let status = parts.next()?;
            let path = parts.next()?;
            Some(DiffFile {
                path: path.to_string(),
                status: status.to_string(),
            })
        })
        .collect()
}

fn emit_status(app: &AppHandle, summary: &TaskSummary) {
    let _ = app.emit("task_status_changed", summary);
}

pub fn handle_select_base_repo(path: String) -> Result<BaseRepoInfo> {
    let repo = PathBuf::from(&path);
    ensure_directory(&repo)?;
    validate_git_repo(&repo)?;
    let canonical_path = repo
        .canonicalize()
        .unwrap_or_else(|_| repo.clone())
        .to_string_lossy()
        .to_string();
    let current_branch = run_git(&repo, ["rev-parse", "--abbrev-ref", "HEAD"])?;
    let head = run_git(&repo, ["rev-parse", "HEAD"])?;
    Ok(BaseRepoInfo {
        path,
        canonical_path,
        current_branch,
        head,
    })
}
