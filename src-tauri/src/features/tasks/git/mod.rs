pub mod commands;

use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum DiffMode {
    Worktree,
    Branch,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DiffFile {
    pub path: String,
    pub status: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffPayloadResult {
    pub files: Vec<DiffFile>,
    pub diff: String,
}

pub fn list_branches(path: &Path) -> Result<Vec<String>> {
    let mut args = vec!["branch".to_string(), "--all".to_string(), "--format".to_string()];
    args.push("%(refname:short)".to_string());
    let output = run_git(path, args)?;
    let mut branches: Vec<String> = output
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .filter(|line| !line.contains("HEAD"))
        .collect();
    branches.sort();
    branches.dedup();
    Ok(branches)
}

pub fn git_commit(repo: &Path, message: &str, stage_all: bool) -> Result<()> {
    if stage_all {
        run_git(repo, ["add", "-A"])?;
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
    base_commit: &str,
    ignore_whitespace: Option<&str>,
) -> Result<DiffPayloadResult> {
    let mut diff_args = vec!["diff".to_string()];
    if let Some(flag) = ignore_whitespace {
        diff_args.push(flag.to_string());
    }
    diff_args.push(base_commit.to_string());
    let diff = run_git(repo, diff_args)?;

    let mut files_args = vec!["diff", "--name-status"].into_iter().map(|s| s.to_string()).collect::<Vec<String>>();
    if let Some(flag) = ignore_whitespace {
        files_args.push(flag.to_string());
    }
    files_args.push(base_commit.to_string());
    let files_output = run_git(repo, files_args)?;
    let files = files_output
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
        .collect();

    Ok(DiffPayloadResult { files, diff })
}

pub fn run_git<I, S>(repo: &Path, args: I) -> Result<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        Err(crate::error::TaskError::Message(stderr).into())
    }
}

pub fn get_repo_root(path: &Path) -> Result<std::path::PathBuf> {
    let output = run_git(path, ["rev-parse", "--show-toplevel"])?;
    Ok(std::path::PathBuf::from(output.trim()))
}

pub fn validate_git_repo(path: &Path) -> Result<()> {
    let _ = run_git(path, ["rev-parse", "--is-inside-work-tree"])?;
    Ok(())
}

pub fn list_worktrees(repo_root: &Path) -> Result<Vec<WorktreeEntry>> {
    let output = run_git(repo_root, ["worktree", "list", "--porcelain"])?;
    let mut entries = Vec::new();
    let mut current = WorktreeEntry::default();
    for line in output.lines() {
        if line.starts_with("worktree ") {
            if !current.path.as_os_str().is_empty() {
                entries.push(current.clone());
                current = WorktreeEntry::default();
            }
            current.path = std::path::PathBuf::from(line.trim_start_matches("worktree ").trim());
        } else if let Some(branch) = line.strip_prefix("branch ") {
            current.branch = Some(branch.trim().to_string());
        } else if let Some(head) = line.strip_prefix("HEAD ") {
            current.head = head.trim().to_string();
        }
    }
    if !current.path.as_os_str().is_empty() {
        entries.push(current);
    }
    Ok(entries)
}

#[derive(Debug, Default, Clone)]
pub struct WorktreeEntry {
    pub path: std::path::PathBuf,
    pub branch: Option<String>,
    pub head: String,
}
