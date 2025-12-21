use crate::error::{Result, TaskError};
use crate::utils::fs::ensure_directory;
use serde::{Deserialize, Serialize};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;
#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[derive(Debug, Clone)]
pub struct WorktreeEntry {
    pub path: PathBuf,
    pub head: String,
    pub branch: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffFile {
    pub path: String,
    pub status: String,
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

pub fn validate_git_repo(path: &Path) -> Result<()> {
    run_git(path, ["rev-parse", "--show-toplevel"]).map(|_| ())
}

pub fn get_repo_root(path: &Path) -> Result<PathBuf> {
    let root = run_git(path, ["rev-parse", "--show-toplevel"])?;
    Ok(PathBuf::from(root))
}

pub fn list_branches(path: &Path) -> Result<Vec<String>> {
    ensure_directory(path)?;
    validate_git_repo(path)?;
    let output = run_git(path, ["branch", "--format=%(refname:short)"])?;
    let branches = output
        .lines()
        .map(|line| line.trim().trim_start_matches('*').trim().to_string())
        .filter(|line| !line.is_empty())
        .collect();
    Ok(branches)
}

pub fn list_worktrees(repo: &Path) -> Result<Vec<WorktreeEntry>> {
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

pub struct DiffResult {
    pub diff: String,
    pub files: Vec<DiffFile>,
}

pub fn git_commit(repo: &Path, message: &str, stage_all: bool) -> Result<()> {
    if stage_all {
        let _ = run_git(repo, ["add", "-A"])?;
    }
    run_git(repo, ["commit", "-m", message]).map(|_| ())
}

pub fn git_push(
    repo: &Path,
    remote: &str,
    branch: &str,
    set_upstream: bool,
) -> Result<()> {
    if set_upstream {
        run_git(repo, ["push", "-u", remote, branch]).map(|_| ())
    } else {
        run_git(repo, ["push", remote, branch]).map(|_| ())
    }
}

pub fn git_diff(
    repo: &Path,
    mode: Option<&str>,
    base_commit: &str,
    whitespace_flag: Option<&str>,
) -> Result<DiffResult> {
    let mut diff_args = vec![
        "-c".to_string(),
        "diff.external=".to_string(),
        "-c".to_string(),
        "pager.diff=false".to_string(),
        "diff".to_string(),
    ];
    if let Some(flag) = whitespace_flag {
        diff_args.push(flag.to_string());
    }
    if let Some(mode_flag) = mode {
        diff_args.push(mode_flag.to_string());
    }
    diff_args.push(base_commit.to_string());
    let diff_output = run_git(repo, diff_args.iter().map(String::as_str))?;

    let mut files_args = vec![
        "-c".to_string(),
        "diff.external=".to_string(),
        "-c".to_string(),
        "pager.diff=false".to_string(),
        "diff".to_string(),
        "--name-status".to_string(),
    ];
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

pub fn git_diff_branch(
    repo: &Path,
    base_commit: &str,
    whitespace_flag: Option<&str>,
) -> Result<DiffResult> {
    let mut diff_args = vec![
        "-c".to_string(),
        "diff.external=".to_string(),
        "-c".to_string(),
        "pager.diff=false".to_string(),
        "diff".to_string(),
    ];
    if let Some(flag) = whitespace_flag {
        diff_args.push(flag.to_string());
    }
    diff_args.push(base_commit.to_string());
    let diff_output = run_git(repo, diff_args.iter().map(String::as_str))?;

    let mut files_args = vec![
        "-c".to_string(),
        "diff.external=".to_string(),
        "-c".to_string(),
        "pager.diff=false".to_string(),
        "diff".to_string(),
        "--name-status".to_string(),
    ];
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

pub fn run_git<I, S>(repo: &Path, args: I) -> Result<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args_vec: Vec<OsString> = args
        .into_iter()
        .map(|a| a.as_ref().to_os_string())
        .collect();
    let mut command = Command::new("git");
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let output = command
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
