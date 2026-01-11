pub mod git;
pub mod models;
mod diff;
mod events;
mod repo;
mod worktree;

pub use models::{
    BaseRepoInfo, CommitTaskRequest, CreateTaskRequest, DiffPayload, DiffRequest,
    DiscardTaskRequest, PushTaskRequest, StartTaskRequest, StopTaskRequest, TaskActionRequest,
    TaskStatus, TaskSummary, TerminalResizeRequest, TerminalWriteRequest,
};
pub use repo::handle_select_base_repo;

use crate::agents::{Agent, AgentCallbacks, AgentRuntime, ChildHandle};
use crate::agents::codex::CodexAgent;
use crate::error::{Result, TaskError};
use crate::launcher;
use crate::tasks::git::{
    git_commit, git_diff, git_diff_branch, git_push, get_repo_root, list_worktrees, run_git,
    validate_git_repo,
};
use diff::merge_diff_files;
use events::{emit_status, emit_terminal_exit, emit_terminal_output};
use crate::utils::fs::ensure_directory;
use chrono::Utc;
use log::{debug, info, warn};
use parking_lot::{Mutex, RwLock};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::AppHandle;
use uuid::Uuid;
use worktree::{clean_branch_name, format_title_from_branch, managed_worktree_root};

#[cfg(target_os = "windows")]
fn strip_alt_screen_sequences(input: &str) -> String {
    input
        .replace("\u{1b}[?1049h", "")
        .replace("\u{1b}[?1049l", "")
        .replace("\u{1b}[?1047h", "")
        .replace("\u{1b}[?1047l", "")
        .replace("\u{1b}[?47h", "")
        .replace("\u{1b}[?47l", "")
}

#[cfg(not(target_os = "windows"))]
fn strip_alt_screen_sequences(input: &str) -> String {
    input.to_string()
}


const DEFAULT_SCREEN_ROWS: usize = 40;
const DEFAULT_SCREEN_COLS: usize = 120;

type MasterHandle = Arc<Mutex<Box<dyn portable_pty::MasterPty + Send>>>;

type WriteHandle = Arc<Mutex<Box<dyn Write + Send>>>;

pub use git::{DiffFile, DiffMode};


struct TaskRecord {
    agent: Box<dyn Agent>,
    summary: TaskSummary,
    runtime: Option<TaskRuntime>,
    terminal_buffer: String,
}

struct TaskRuntime {
    child: Arc<Mutex<ChildHandle>>,
    writer: WriteHandle,
    master: MasterHandle,
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
        info!("create_task task_id={} branch={}", task_id, branch_name);

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
            status: TaskStatus::Stopped,
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
                agent: Box::new(CodexAgent::default()),
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
        info!("start_task task_id={} title={}", task_id, title);

        let status_manager = self.clone();
        let status_app = app.clone();
        let output_manager = self.clone();
        let output_app = app.clone();
        let exit_manager = self.clone();
        let exit_app = app.clone();
        let callbacks = AgentCallbacks {
            on_output: Arc::new(move |chunk: String| {
                output_manager.handle_agent_output(task_id, chunk, &output_app);
            }),
            on_status: Arc::new(move |status: TaskStatus| {
                status_manager.handle_agent_status(task_id, status, &status_app);
            }),
            on_exit: Arc::new(move |exit_code: i32| {
                exit_manager.handle_agent_exit(task_id, exit_code, &exit_app);
            }),
        };

        let agent_runtime = {
            let mut tasks = self.inner.tasks.write();
            let record = tasks
                .get_mut(&task_id)
                .ok_or(TaskError::NotFound)?;
            record.agent.reset(DEFAULT_SCREEN_ROWS, DEFAULT_SCREEN_COLS);
            record
                .agent
                .start(&worktree_path, codex_args, env, callbacks)
                .with_context(|| format!("failed to start Codex for task {}", title))?
        };

        let AgentRuntime {
            child,
            writer,
            master,
        } = agent_runtime;

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
            emit_status(app, &record.summary);
        }

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
        info!("stop_task task_id={}", task_id);
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
            record.summary.status = TaskStatus::Stopped;
            emit_status(app, &record.summary);
            return Ok(record.summary.clone());
        }
    }

    pub fn discard_task(&self, req: DiscardTaskRequest, app: &AppHandle) -> Result<()> {
        let task_id = req.task_id;
        info!("discard_task task_id={}", task_id);
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
        debug!("terminal_write task_id={} bytes={}", task_id, req.data.len());
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
            .with_context(|| "failed to write to terminal")?;
        writer_guard.flush().ok();
        Ok(())
    }

    pub fn terminal_resize(&self, req: TerminalResizeRequest) -> Result<()> {
        let task_id = req.task_id;
        debug!("terminal_resize task_id={} rows={} cols={}", task_id, req.rows, req.cols);
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
            .resize(portable_pty::PtySize {
                cols: req.cols,
                rows: req.rows,
                pixel_width: 0,
                pixel_height: 0,
            })
            .with_context(|| "failed to resize terminal")?;
        {
            let mut tasks = self.inner.tasks.write();
            if let Some(record) = tasks.get_mut(&task_id) {
                record
                    .agent
                    .resize(req.rows as usize, req.cols as usize);
            }
        }
        Ok(())
    }

    pub fn get_diff(&self, req: DiffRequest) -> Result<DiffPayload> {
        let task_id = req.task_id;
        debug!("get_diff task_id={} mode={:?}", task_id, req.mode);
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

    pub fn commit_task(&self, req: CommitTaskRequest) -> Result<()> {
        let task_id = req.task_id;
        let message = req.message.trim();
        if message.is_empty() {
            return Err(TaskError::Message("Commit message is required.".into()));
        }
        let stage_all = req.stage_all.unwrap_or(true);
        let worktree_path = self.worktree_path(task_id)?;
        debug!(
            "commit_task task_id={} stage_all={} message_len={}",
            task_id,
            stage_all,
            message.len()
        );
        git_commit(worktree_path.as_path(), message, stage_all)
    }

    pub fn push_task(&self, req: PushTaskRequest) -> Result<()> {
        let task_id = req.task_id;
        let (worktree_path, branch_name) = {
            let tasks = self.inner.tasks.read();
            let record = tasks.get(&task_id).ok_or(TaskError::NotFound)?;
            (
                PathBuf::from(&record.summary.worktree_path),
                record.summary.branch_name.clone(),
            )
        };
        let remote = req.remote.unwrap_or_else(|| "origin".to_string());
        let branch = req.branch.unwrap_or(branch_name);
        let set_upstream = req.set_upstream.unwrap_or(true);
        debug!(
            "push_task task_id={} remote={} branch={} set_upstream={}",
            task_id, remote, branch, set_upstream
        );
        git_push(worktree_path.as_path(), remote.as_str(), branch.as_str(), set_upstream)
    }

    fn apply_agent_status(&self, record: &mut TaskRecord, status: TaskStatus, app: &AppHandle) {
        if record.summary.status != status {
            record.summary.status = status;
            emit_status(app, &record.summary);
        }
    }

    pub fn handle_agent_status(&self, task_id: Uuid, status: TaskStatus, app: &AppHandle) {
        debug!("agent_status task_id={} status={:?}", task_id, status);
        let mut tasks = self.inner.tasks.write();
        if let Some(record) = tasks.get_mut(&task_id) {
            self.apply_agent_status(record, status, app);
        }
    }

    pub fn handle_agent_output(&self, task_id: Uuid, chunk: String, app: &AppHandle) {
        let chunk = strip_alt_screen_sequences(&chunk);
        debug!("agent_output task_id={} bytes={}", task_id, chunk.len());
        let mut tasks = self.inner.tasks.write();
        if let Some(record) = tasks.get_mut(&task_id) {
            record.terminal_buffer.push_str(&chunk);
        }
        emit_terminal_output(app, task_id, chunk);
    }

    pub fn handle_agent_exit(&self, task_id: Uuid, exit_code: i32, app: &AppHandle) {
        info!("agent_exit task_id={} exit_code={}", task_id, exit_code);
        let _ = self.finish_task(task_id, exit_code, app);
        emit_terminal_exit(app, task_id, exit_code);
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
        debug!("register_existing_worktrees base_repo_path={}", base_repo_path);
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
                status: TaskStatus::Stopped,
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
                    agent: Box::new(CodexAgent::default()),
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
        debug!("open_in_vscode task_id={} path={}", req.task_id, path.display());
        launcher::open_path_in_vscode(path.as_path())
    }

    pub fn open_terminal(&self, req: TaskActionRequest) -> Result<()> {
        let path = self.worktree_path(req.task_id)?;
        debug!("open_terminal task_id={} path={}", req.task_id, path.display());
        launcher::open_path_terminal(path.as_path())
    }

    fn finish_task(&self, task_id: Uuid, exit_code: i32, app: &AppHandle) -> Result<()> {
        let mut tasks = self.inner.tasks.write();
        let record = tasks
            .get_mut(&task_id)
            .ok_or(TaskError::NotFound)?;
        if record.runtime.is_none() {
            warn!("finish_task task_id={} without runtime", task_id);
        }
        record.summary.exit_code = Some(exit_code);
        record.summary.ended_at = Some(Utc::now());
        record.runtime = None;
        let target_status = match record.summary.status {
            TaskStatus::Stopped => TaskStatus::Stopped,
            TaskStatus::Discarded => TaskStatus::Discarded,
            _ if exit_code == 0 => TaskStatus::Completed,
            _ => TaskStatus::Failed,
        };
        record.summary.status = target_status;
        emit_status(app, &record.summary);
        Ok(())
    }
}

use anyhow::Context;
