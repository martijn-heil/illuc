pub mod commands;

use crate::error::{Result, TaskError};
use crate::features::tasks::{DiffLine, DiffLineType};
use git2::build::CheckoutBuilder;
use git2::{
    BranchType, Cred, Delta, DiffFormat, DiffOptions, ErrorCode, FetchOptions, IndexAddOption,
    ObjectType, PushOptions, RemoteCallbacks, Repository, ResetType, Signature, Status,
    StatusOptions, WorktreeAddOptions, WorktreePruneOptions,
};
use log::warn;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

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
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffPayloadResult {
    pub files: Vec<DiffFile>,
}

#[derive(Debug, Default, Clone)]
pub struct WorktreeEntry {
    pub path: PathBuf,
    pub branch: Option<String>,
    pub head: String,
}

fn map_git_err(err: git2::Error) -> TaskError {
    TaskError::Message(err.message().to_string())
}

fn open_repo(path: &Path) -> Result<Repository> {
    let repo = Repository::discover(path).map_err(map_git_err)?;
    configure_windows_long_paths(&repo);
    Ok(repo)
}

#[cfg(target_os = "windows")]
fn configure_windows_long_paths(repo: &Repository) {
    match repo.config() {
        Ok(mut config) => {
            if let Err(error) = config.set_bool("core.longpaths", true) {
                warn!(
                    "failed to set core.longpaths=true for repository: {}",
                    error
                );
            }
        }
        Err(error) => {
            warn!(
                "failed to open repository config to set core.longpaths: {}",
                error
            );
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn configure_windows_long_paths(_repo: &Repository) {}

fn build_remote_callbacks(repo: &Repository) -> Result<RemoteCallbacks<'static>> {
    // RemoteCallbacks owns the credential callback. We move a cloned config into it
    // to allow Cred::credential_helper to function.
    let config = repo.config().map_err(map_git_err)?;
    let ssh_keys = default_ssh_private_keys();
    let mut callbacks = RemoteCallbacks::new();

    let mut tried_ssh_agent = false;
    let mut next_ssh_key_index = 0;
    let mut tried_credential_helper = false;
    #[cfg(target_os = "windows")]
    let mut tried_gcm = false;
    let mut tried_username = false;
    let mut tried_default = false;

    callbacks.credentials(move |url, username_from_url, allowed| {
        if allowed.is_ssh_key() {
            if !tried_ssh_agent {
                tried_ssh_agent = true;
                if let Some(username) = username_from_url {
                    if let Ok(cred) = Cred::ssh_key_from_agent(username) {
                        return Ok(cred);
                    }
                }
            }

            if let Some(username) = username_from_url {
                while let Some(private_key) = ssh_keys.get(next_ssh_key_index) {
                    next_ssh_key_index += 1;
                    if let Ok(cred) = Cred::ssh_key(username, None, private_key, None) {
                        return Ok(cred);
                    }
                }
            }
        }

        if allowed.is_user_pass_plaintext() {
            if !tried_credential_helper {
                tried_credential_helper = true;
                if let Ok(cred) = Cred::credential_helper(&config, url, username_from_url) {
                    return Ok(cred);
                }
            }
            #[cfg(target_os = "windows")]
            if !tried_gcm {
                tried_gcm = true;
                if let Some(cred) = try_gcm_credential(url, username_from_url) {
                    return Ok(cred);
                }
            }
            return Err(git2::Error::from_str(
                "No git credentials configured. Configure credential.helper (e.g. manager-core) or set up SSH.",
            ));
        }

        if allowed.is_username() && !tried_username {
            tried_username = true;
            if let Some(username) = username_from_url {
                return Cred::username(username);
            }
        }

        if allowed.is_default() && !tried_default {
            tried_default = true;
            return Cred::default();
        }

        Err(git2::Error::from_str(
            "No supported credential methods available. Configure SSH or a credential helper.",
        ))
    });
    Ok(callbacks)
}

fn default_ssh_private_keys() -> Vec<PathBuf> {
    //
    // How should home dir be determined when running on windows?
    // 1. Always windows home?
    // 2. Always WSL home?
    // 3. Both? (meaning ssh keys from both directories will be tried)
    //
    let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) else {
        return Vec::new();
    };
    let ssh_dir = PathBuf::from(home).join(".ssh");
    ["id_ed25519", "id_ecdsa", "id_rsa"]
        .into_iter()
        .map(|name| ssh_dir.join(name))
        .filter(|path| path.is_file())
        .collect()
}

pub fn validate_git_repo(path: &Path) -> Result<()> {
    match Repository::discover(path) {
        Ok(repo) => {
            configure_windows_long_paths(&repo);
            Ok(())
        }
        Err(err) if err.code() == ErrorCode::NotFound => Err(TaskError::Message(
            "The selected directory is not a Git repository.".to_string(),
        )),
        Err(err) => Err(map_git_err(err)),
    }
}

pub fn get_repo_root(path: &Path) -> Result<PathBuf> {
    let repo = open_repo(path)?;
    if let Some(workdir) = repo.workdir() {
        Ok(workdir.to_path_buf())
    } else {
        Ok(repo.path().to_path_buf())
    }
}

pub fn resolve_commit_id(path: &Path, rev: &str) -> Result<String> {
    let repo = open_repo(path)?;
    let object = repo.revparse_single(rev).map_err(map_git_err)?;
    let commit = object.peel_to_commit().map_err(map_git_err)?;
    Ok(commit.id().to_string())
}

pub fn get_head_commit(path: &Path) -> Result<String> {
    let repo = open_repo(path)?;
    let head = repo.head().map_err(map_git_err)?;
    let oid = head
        .target()
        .ok_or_else(|| TaskError::Message("HEAD is not a commit.".into()))?;
    Ok(oid.to_string())
}

pub fn get_head_branch(path: &Path) -> Result<String> {
    let repo = open_repo(path)?;
    let head = repo.head().map_err(map_git_err)?;
    Ok(head.shorthand().unwrap_or("HEAD").to_string())
}

pub fn list_branches(path: &Path) -> Result<Vec<String>> {
    let repo = open_repo(path)?;
    let mut branches = Vec::new();
    let iter = repo.branches(None).map_err(map_git_err)?;
    for branch in iter {
        let (branch, _) = branch.map_err(map_git_err)?;
        let name = branch.name().map_err(map_git_err)?.unwrap_or("HEAD");
        if !name.contains("HEAD") {
            branches.push(name.to_string());
        }
    }
    branches.sort();
    branches.dedup();
    Ok(branches)
}

fn split_remote_tracking(input: &str) -> Option<(String, String)> {
    // Accept "origin/main" or "upstream/feature/foo".
    // We intentionally do not accept raw OIDs or revspecs here.
    let mut parts = input.splitn(2, '/');
    let remote = parts.next()?.trim();
    let branch = parts.next()?.trim();
    if remote.is_empty() || branch.is_empty() {
        return None;
    }
    Some((remote.to_string(), branch.to_string()))
}

fn upstream_remote_and_branch(repo: &Repository, local_branch: &str) -> Result<(String, String)> {
    let local_ref = format!("refs/heads/{}", local_branch);
    if let Ok(upstream) = repo.branch_upstream_name(&local_ref) {
        if let Some(upstream) = upstream.as_str() {
            // Expected: refs/remotes/<remote>/<branch>
            let parts: Vec<&str> = upstream.split('/').collect();
            if parts.len() >= 4 && parts[0] == "refs" && parts[1] == "remotes" {
                return Ok((parts[2].to_string(), parts[3..].join("/")));
            }
        }
    }
    Ok(("origin".to_string(), local_branch.to_string()))
}

fn fetch_branch(repo: &Repository, remote_name: &str, branch: &str) -> Result<()> {
    let mut remote = repo.find_remote(remote_name).map_err(map_git_err)?;
    let callbacks = build_remote_callbacks(repo)?;
    let mut fetch_options = FetchOptions::new();
    fetch_options.remote_callbacks(callbacks);
    remote
        .fetch(&[branch], Some(&mut fetch_options), None)
        .map_err(map_git_err)?;
    Ok(())
}

fn fast_forward_local_branch_if_behind(
    repo: &Repository,
    repo_root: &Path,
    branch: &str,
    remote_name: &str,
    remote_branch: &str,
) -> Result<()> {
    let local_ref = format!("refs/heads/{}", branch);
    let remote_ref = format!("refs/remotes/{}/{}", remote_name, remote_branch);

    let local_oid = match repo.refname_to_id(&local_ref) {
        Ok(oid) => oid,
        Err(err) => {
            warn!(
                "failed to resolve local branch ref {} for fast-forward check: {}",
                local_ref, err
            );
            return Ok(());
        }
    };
    let remote_oid = match repo.refname_to_id(&remote_ref) {
        Ok(oid) => oid,
        Err(err) => {
            warn!(
                "failed to resolve remote branch ref {} for fast-forward check: {}",
                remote_ref, err
            );
            return Ok(());
        }
    };

    if local_oid == remote_oid {
        return Ok(());
    }

    // Only fast-forward when the remote is a descendant of local (local behind).
    // If local is ahead or diverged, we do nothing (local changes must not be overwritten).
    let remote_descends_local = repo
        .graph_descendant_of(remote_oid, local_oid)
        .map_err(map_git_err)?;
    if !remote_descends_local {
        return Ok(());
    }

    let head = repo.head().map_err(map_git_err)?;
    let on_branch = head.is_branch() && head.shorthand() == Some(branch);
    let update_checkout = on_branch && repo.workdir().is_some();
    if update_checkout && has_uncommitted_changes(repo_root).unwrap_or(true) {
        warn!(
            "skipping fast-forward of checked-out base branch {} because the main worktree is dirty",
            branch
        );
        return Ok(());
    }

    {
        let mut reference = repo.find_reference(&local_ref).map_err(map_git_err)?;
        reference
            .set_target(remote_oid, "illuc: fast-forward base branch")
            .map_err(map_git_err)?;
    }

    if update_checkout {
        let object = repo
            .find_object(remote_oid, Some(ObjectType::Commit))
            .map_err(map_git_err)?;
        repo.reset(&object, ResetType::Hard, None)
            .map_err(map_git_err)?;
    }

    Ok(())
}

pub fn fetch_base_branch_best_effort(repo_root: &Path, base_ref: &str) -> Result<()> {
    // Best-effort "fetch the base branch so we base worktrees on the latest remote state".
    // We only fast-forward local branches when it's safe (local behind). We never overwrite
    // local commits (local ahead/diverged => no-op).
    let repo = open_repo(repo_root)?;

    let base_ref = base_ref.trim();
    if base_ref.is_empty() {
        return Ok(());
    }

    // If base_ref is "HEAD", treat the currently checked out branch as the base branch.
    if base_ref == "HEAD" {
        let head = repo.head().map_err(map_git_err)?;
        if !(head.is_branch()) {
            return Ok(());
        }
        let branch = match head.shorthand() {
            Some(name) if !name.trim().is_empty() => name.trim().to_string(),
            _ => return Ok(()),
        };
        let (remote_name, remote_branch) = upstream_remote_and_branch(&repo, &branch)?;
        fetch_branch(&repo, &remote_name, &remote_branch)?;
        fast_forward_local_branch_if_behind(
            &repo,
            repo_root,
            &branch,
            &remote_name,
            &remote_branch,
        )?;
        return Ok(());
    }

    // If base_ref is a local branch name (or refs/heads/<name>), fetch its upstream and ff-only.
    let local_branch = if let Some(name) = base_ref.strip_prefix("refs/heads/") {
        Some(name.trim().to_string())
    } else if repo.find_branch(base_ref, BranchType::Local).is_ok() {
        Some(base_ref.to_string())
    } else {
        None
    };
    if let Some(branch) = local_branch {
        let (remote_name, remote_branch) = upstream_remote_and_branch(&repo, &branch)?;
        fetch_branch(&repo, &remote_name, &remote_branch)?;
        fast_forward_local_branch_if_behind(
            &repo,
            repo_root,
            &branch,
            &remote_name,
            &remote_branch,
        )?;
        return Ok(());
    }

    // If base_ref looks like a remote-tracking ref ("origin/foo"), just fetch it.
    // We intentionally do not move any local branch in this case.
    if let Some((remote_name, remote_branch)) = split_remote_tracking(base_ref) {
        fetch_branch(&repo, &remote_name, &remote_branch)?;
        return Ok(());
    }

    Ok(())
}

pub fn add_worktree(
    repo_root: &Path,
    branch_name: &str,
    worktree_path: &Path,
    base_ref: &str,
    worktree_name: &str,
) -> Result<()> {
    let repo = open_repo(repo_root)?;
    if let Err(err) = prune_stale_worktrees(&repo) {
        warn!(
            "failed to prune stale worktrees before adding {}: {}",
            branch_name, err
        );
    }
    let base_object = repo.revparse_single(base_ref).map_err(map_git_err)?;
    let base_commit = base_object.peel_to_commit().map_err(map_git_err)?;
    if repo.find_branch(branch_name, BranchType::Local).is_err() {
        repo.branch(branch_name, &base_commit, false)
            .map_err(map_git_err)?;
    }
    create_worktree_for_branch(&repo, branch_name, worktree_path, worktree_name).or_else(|err| {
        if !is_reference_checked_out_error(&err) {
            return Err(map_git_err(err));
        }
        warn!(
            "worktree add for {} failed because the ref is already checked out; pruning stale worktrees and retrying",
            branch_name
        );
        if let Err(prune_err) = prune_stale_worktrees(&repo) {
            warn!(
                "failed to prune stale worktrees before retrying {}: {}",
                branch_name, prune_err
            );
        }
        create_worktree_for_branch(&repo, branch_name, worktree_path, worktree_name)
            .map_err(map_git_err)
    })?;
    ensure_relative_worktree_gitdir(worktree_path)?;
    Ok(())
}

fn create_worktree_for_branch(
    repo: &Repository,
    branch_name: &str,
    worktree_path: &Path,
    worktree_name: &str,
) -> std::result::Result<(), git2::Error> {
    let reference_name = format!("refs/heads/{}", branch_name);
    let reference = repo.find_reference(&reference_name)?;
    let mut options = WorktreeAddOptions::new();
    options.reference(Some(&reference));
    repo.worktree(worktree_name, worktree_path, Some(&options))?;
    Ok(())
}

fn is_reference_checked_out_error(err: &git2::Error) -> bool {
    err.message().contains(" is already checked out")
}

pub fn ensure_relative_worktree_gitdir(worktree_path: &Path) -> Result<()> {
    let git_file_path = worktree_path.join(".git");
    let git_file = std::fs::read_to_string(&git_file_path)
        .map_err(|error| TaskError::Message(error.to_string()))?;
    let Some(gitdir) = git_file
        .lines()
        .find_map(|line| line.trim().strip_prefix("gitdir:").map(str::trim))
    else {
        return Ok(());
    };

    let gitdir_path = normalize_relative_path_input(Path::new(gitdir));
    if !gitdir_path.is_absolute() {
        return Ok(());
    }

    let worktree_root = std::fs::canonicalize(worktree_path)
        .map_err(|error| TaskError::Message(error.to_string()))?;
    let worktree_root = normalize_relative_path_input(&worktree_root);
    let relative_gitdir = diff_paths(&gitdir_path, &worktree_root).ok_or_else(|| {
        TaskError::Message(format!(
            "failed to relativize gitdir {} from worktree {}",
            gitdir_path.display(),
            worktree_root.display()
        ))
    })?;

    let relative_gitdir = relative_gitdir.to_string_lossy().replace('\\', "/");
    std::fs::write(&git_file_path, format!("gitdir: {relative_gitdir}\n"))
        .map_err(|error| TaskError::Message(error.to_string()))?;
    Ok(())
}

fn diff_paths(path: &Path, base: &Path) -> Option<PathBuf> {
    let path_components: Vec<_> = path.components().collect();
    let base_components: Vec<_> = base.components().collect();

    let mut common_length = 0usize;
    while common_length < path_components.len()
        && common_length < base_components.len()
        && path_components[common_length] == base_components[common_length]
    {
        common_length += 1;
    }

    let path_prefix = path_components
        .iter()
        .take_while(|component| matches!(component, Component::Prefix(_) | Component::RootDir))
        .count();
    let base_prefix = base_components
        .iter()
        .take_while(|component| matches!(component, Component::Prefix(_) | Component::RootDir))
        .count();
    if common_length < path_prefix.max(base_prefix) {
        return None;
    }

    let mut relative = PathBuf::new();
    for component in &base_components[common_length..] {
        if matches!(component, Component::Normal(_) | Component::ParentDir) {
            relative.push("..");
        }
    }
    for component in &path_components[common_length..] {
        match component {
            Component::Normal(value) => relative.push(value),
            Component::CurDir => {}
            Component::ParentDir => relative.push(".."),
            Component::Prefix(_) | Component::RootDir => return None,
        }
    }

    if relative.as_os_str().is_empty() {
        relative.push(".");
    }
    Some(relative)
}

#[cfg(target_os = "windows")]
fn normalize_relative_path_input(path: &Path) -> PathBuf {
    let value = path.to_string_lossy().replace('/', "\\");
    let value = value
        .strip_prefix(r"\\?\")
        .map(str::to_string)
        .unwrap_or(value);
    PathBuf::from(value)
}

#[cfg(not(target_os = "windows"))]
fn normalize_relative_path_input(path: &Path) -> PathBuf {
    path.to_path_buf()
}

pub fn remove_worktree(repo_root: &Path, worktree_path: &Path) -> Result<()> {
    let repo = open_repo(repo_root)?;
    let worktrees = repo.worktrees().map_err(map_git_err)?;
    for name in worktrees.iter().flatten() {
        let worktree = repo.find_worktree(name).map_err(map_git_err)?;
        if worktree.path() == worktree_path {
            worktree.prune(None).map_err(map_git_err)?;
            return Ok(());
        }
    }
    Ok(())
}

pub fn delete_branch(repo_root: &Path, branch_name: &str) -> Result<()> {
    let repo = open_repo(repo_root)?;
    if let Ok(mut branch) = repo.find_branch(branch_name, BranchType::Local) {
        branch.delete().map_err(map_git_err)?;
    }
    Ok(())
}

pub fn list_worktrees(repo_root: &Path) -> Result<Vec<WorktreeEntry>> {
    let repo = open_repo(repo_root)?;
    let worktrees = repo.worktrees().map_err(map_git_err)?;
    let mut entries = Vec::new();
    for name in worktrees.iter().flatten() {
        let worktree = match repo.find_worktree(name) {
            Ok(worktree) => worktree,
            Err(err) => {
                warn!(
                    "skipping worktree {}: failed to load metadata: {}",
                    name, err
                );
                continue;
            }
        };
        let path = worktree.path();
        let worktree_repo = match Repository::open(path) {
            Ok(repo) => {
                configure_windows_long_paths(&repo);
                repo
            }
            Err(err) => {
                warn!(
                    "skipping worktree {} at {}: cannot open repository: {}",
                    name,
                    path.display(),
                    err
                );
                continue;
            }
        };
        let head = match worktree_repo.head() {
            Ok(head) => head,
            Err(err) => {
                warn!(
                    "skipping worktree {} at {}: cannot resolve HEAD: {}",
                    name,
                    path.display(),
                    err
                );
                continue;
            }
        };
        let head_oid = match head.target() {
            Some(oid) => oid,
            None => {
                warn!(
                    "skipping worktree {} at {}: HEAD is not a commit",
                    name,
                    path.display()
                );
                continue;
            }
        };
        let branch = if head.is_branch() {
            head.shorthand().map(|name| name.to_string())
        } else {
            None
        };
        entries.push(WorktreeEntry {
            path: path.to_path_buf(),
            branch,
            head: head_oid.to_string(),
        });
    }
    Ok(entries)
}

pub fn prune_worktrees(repo_root: &Path) -> Result<()> {
    let repo = open_repo(repo_root)?;
    prune_stale_worktrees(&repo)
}

fn prune_stale_worktrees(repo: &Repository) -> Result<()> {
    let worktrees = repo.worktrees().map_err(map_git_err)?;
    for name in worktrees.iter().flatten() {
        if let Ok(worktree) = repo.find_worktree(name) {
            let mut options = WorktreePruneOptions::new();
            if let Err(err) = worktree.prune(Some(&mut options)) {
                if err.message().contains("not pruning valid working tree") {
                    continue;
                }
                warn!("failed to prune worktree {}: {}", name, err);
            }
        } else {
            warn!("failed to find worktree {} while pruning", name);
        }
    }
    Ok(())
}

pub fn git_commit(repo: &Path, message: &str, stage_all: bool) -> Result<()> {
    let repo = open_repo(repo)?;
    let mut index = repo.index().map_err(map_git_err)?;
    if stage_all {
        index
            .add_all(["*"].iter(), IndexAddOption::DEFAULT, None)
            .map_err(map_git_err)?;
    }
    index.write().map_err(map_git_err)?;
    let tree_id = index.write_tree().map_err(map_git_err)?;
    let tree = repo.find_tree(tree_id).map_err(map_git_err)?;

    let signature = repo
        .signature()
        .or_else(|_| Signature::now("illuc", "illuc@local").map_err(map_git_err))?;

    let mut parents = Vec::new();
    if let Ok(head) = repo.head() {
        if let Some(oid) = head.target() {
            if let Ok(parent) = repo.find_commit(oid) {
                parents.push(parent);
            }
        }
    }
    let parent_refs: Vec<&git2::Commit> = parents.iter().collect();

    repo.commit(
        Some("HEAD"),
        &signature,
        &signature,
        message,
        &tree,
        &parent_refs,
    )
    .map_err(map_git_err)?;

    Ok(())
}

pub fn stage_all(repo: &Path) -> Result<()> {
    let repo = open_repo(repo)?;
    let mut index = repo.index().map_err(map_git_err)?;
    index
        .add_all(["*"].iter(), IndexAddOption::DEFAULT, None)
        .map_err(map_git_err)?;
    index.write().map_err(map_git_err)?;
    Ok(())
}

pub fn git_push(repo: &Path, remote_name: &str, branch: &str, set_upstream: bool) -> Result<()> {
    let repo = open_repo(repo)?;

    let mut push_options = PushOptions::new();
    push_options.remote_callbacks(build_remote_callbacks(&repo)?);

    let mut remote = repo.find_remote(remote_name).map_err(map_git_err)?;
    let local_ref = if branch.starts_with("refs/") {
        branch.to_string()
    } else {
        format!("refs/heads/{}", branch)
    };
    let refspec = format!("{}:refs/heads/{}", local_ref, branch);
    remote
        .push(&[refspec.as_str()], Some(&mut push_options))
        .map_err(map_git_err)?;

    if set_upstream {
        if let Ok(mut local_branch) = repo.find_branch(branch, BranchType::Local) {
            let upstream = format!("{}/{}", remote_name, branch);
            if let Err(err) = local_branch.set_upstream(Some(&upstream)) {
                warn!(
                    "failed to set upstream for branch {} to {}: {}",
                    branch, upstream, err
                );
            }
        } else {
            warn!(
                "pushed branch {} but could not find local branch to set upstream",
                branch
            );
        }
    }

    Ok(())
}

pub fn git_merge_branch(repo_root: &Path, target_branch: &str, source_branch: &str) -> Result<()> {
    let target_branch = target_branch.trim();
    let source_branch = source_branch.trim();
    if target_branch.is_empty() {
        return Err(TaskError::Message("Target branch is required.".into()));
    }
    if source_branch.is_empty() {
        return Err(TaskError::Message("Source branch is required.".into()));
    }
    if target_branch == source_branch {
        return Err(TaskError::Message(
            "The task branch already matches the selected main branch.".into(),
        ));
    }
    if has_uncommitted_changes(repo_root)? {
        return Err(TaskError::Message(
            "The main repository has uncommitted changes. Commit, stash, or discard them before merging."
                .into(),
        ));
    }

    let repo = open_repo(repo_root)?;
    if repo.state() != git2::RepositoryState::Clean {
        return Err(TaskError::Message(
            "The repository is already in the middle of another Git operation. Resolve that first."
                .into(),
        ));
    }

    checkout_local_branch(&repo, target_branch)?;

    let source_ref = format!("refs/heads/{}", source_branch);
    let source_oid = repo.refname_to_id(&source_ref).map_err(map_git_err)?;
    let source_annotated = repo
        .find_annotated_commit(source_oid)
        .map_err(map_git_err)?;
    let (analysis, _) = repo
        .merge_analysis(&[&source_annotated])
        .map_err(map_git_err)?;

    if analysis.is_up_to_date() {
        return Ok(());
    }

    if analysis.is_fast_forward() {
        fast_forward_branch_to(&repo, target_branch, source_oid)?;
        return Ok(());
    }

    if !analysis.is_normal() {
        return Err(TaskError::Message(
            "Git could not determine a supported merge strategy for this branch pair.".into(),
        ));
    }

    let mut checkout = CheckoutBuilder::new();
    checkout
        .safe()
        .allow_conflicts(true)
        .conflict_style_merge(true);
    repo.merge(&[&source_annotated], None, Some(&mut checkout))
        .map_err(map_git_err)?;

    let mut index = repo.index().map_err(map_git_err)?;
    if index.has_conflicts() {
        return Err(TaskError::Message(format!(
            "Merge conflicts occurred while merging '{}' into '{}'. Resolve them manually in the main repository, then complete or abort the merge there.",
            source_branch, target_branch
        )));
    }

    let tree_id = index.write_tree_to(&repo).map_err(map_git_err)?;
    let tree = repo.find_tree(tree_id).map_err(map_git_err)?;
    let signature = repo
        .signature()
        .or_else(|_| Signature::now("illuc", "illuc@local").map_err(map_git_err))?;
    let target_commit = repo
        .head()
        .and_then(|head| head.peel_to_commit())
        .map_err(map_git_err)?;
    let source_commit = repo.find_commit(source_oid).map_err(map_git_err)?;
    let message = format!("Merge branch '{}' into '{}'", source_branch, target_branch);
    repo.commit(
        Some("HEAD"),
        &signature,
        &signature,
        &message,
        &tree,
        &[&target_commit, &source_commit],
    )
    .map_err(map_git_err)?;
    let mut checkout = CheckoutBuilder::new();
    checkout.safe();
    repo.checkout_head(Some(&mut checkout))
        .map_err(map_git_err)?;
    repo.cleanup_state().map_err(map_git_err)?;
    Ok(())
}

fn checkout_local_branch(repo: &Repository, branch: &str) -> Result<()> {
    repo.find_branch(branch, BranchType::Local)
        .map_err(map_git_err)?;
    let reference_name = format!("refs/heads/{}", branch);
    let object = repo
        .revparse_single(reference_name.as_str())
        .map_err(map_git_err)?;
    let mut checkout = CheckoutBuilder::new();
    checkout.safe();
    repo.checkout_tree(&object, Some(&mut checkout))
        .map_err(map_git_err)?;
    repo.set_head(reference_name.as_str())
        .map_err(map_git_err)?;
    Ok(())
}

fn fast_forward_branch_to(repo: &Repository, branch: &str, oid: git2::Oid) -> Result<()> {
    let reference_name = format!("refs/heads/{}", branch);
    let mut reference = repo
        .find_reference(reference_name.as_str())
        .map_err(map_git_err)?;
    reference
        .set_target(oid, "illuc: fast-forward merge")
        .map_err(map_git_err)?;
    repo.set_head(reference_name.as_str())
        .map_err(map_git_err)?;
    let object = repo
        .find_object(oid, Some(ObjectType::Commit))
        .map_err(map_git_err)?;
    repo.reset(&object, ResetType::Hard, None)
        .map_err(map_git_err)?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn try_gcm_credential(url: &str, username: Option<&str>) -> Option<Cred> {
    let mut config = match git2::Config::new() {
        Ok(config) => config,
        Err(err) => {
            warn!(
                "failed to initialize temporary git config for GCM credentials: {}",
                err
            );
            return None;
        }
    };
    for helper in ["manager-core", "manager"] {
        if let Err(err) = config.set_str("credential.helper", helper) {
            warn!(
                "failed to set git credential.helper={} while probing GCM: {}",
                helper, err
            );
            continue;
        }
        if let Ok(cred) = Cred::credential_helper(&config, url, username) {
            return Some(cred);
        }
    }
    None
}

fn map_delta_status(delta: Delta) -> &'static str {
    match delta {
        Delta::Unmodified => " ",
        Delta::Added => "A",
        Delta::Deleted => "D",
        Delta::Modified => "M",
        Delta::Renamed => "R",
        Delta::Copied => "C",
        Delta::Typechange => "T",
        Delta::Untracked => "?",
        Delta::Ignored => "I",
        Delta::Unreadable => "U",
        Delta::Conflicted => "U",
    }
}

fn map_line_type(origin: char) -> DiffLineType {
    match origin {
        '+' => DiffLineType::Add,
        '-' => DiffLineType::Del,
        ' ' => DiffLineType::Context,
        'H' => DiffLineType::Hunk,
        _ => DiffLineType::Meta,
    }
}

pub fn git_diff(
    repo: &Path,
    base_commit: &str,
    ignore_whitespace: Option<&str>,
) -> Result<DiffPayloadResult> {
    let repo = open_repo(repo)?;
    let base_object = repo.revparse_single(base_commit).map_err(map_git_err)?;
    let base_commit = base_object.peel_to_commit().map_err(map_git_err)?;
    let base_tree = base_commit.tree().map_err(map_git_err)?;

    let mut options = DiffOptions::new();
    if ignore_whitespace.is_some() {
        options.ignore_whitespace(true);
        options.ignore_whitespace_change(true);
        options.ignore_whitespace_eol(true);
    }

    let diff = repo
        .diff_tree_to_workdir_with_index(Some(&base_tree), Some(&mut options))
        .map_err(map_git_err)?;

    let mut files_by_path: HashMap<String, DiffFile> = HashMap::new();
    let mut file_order: Vec<String> = Vec::new();

    for delta in diff.deltas() {
        let status = map_delta_status(delta.status()).to_string();
        let path = match delta.new_file().path().or_else(|| delta.old_file().path()) {
            Some(path) => path.to_string_lossy().to_string(),
            None => continue,
        };
        let entry = files_by_path.entry(path.clone());
        if let std::collections::hash_map::Entry::Vacant(vacant) = entry {
            file_order.push(path.clone());
            vacant.insert(DiffFile {
                path,
                status,
                lines: Vec::new(),
            });
        } else if let Some(file) = files_by_path.get_mut(&path) {
            file.status = status;
        }
    }

    diff.print(DiffFormat::Patch, |delta, _hunk, line| {
        let path = match delta.new_file().path().or_else(|| delta.old_file().path()) {
            Some(path) => path.to_string_lossy().to_string(),
            None => return true,
        };
        if !files_by_path.contains_key(&path) {
            file_order.push(path.clone());
            files_by_path.insert(
                path.clone(),
                DiffFile {
                    path: path.clone(),
                    status: "M".to_string(),
                    lines: Vec::new(),
                },
            );
        }
        let content = String::from_utf8_lossy(line.content());
        let content = content.trim_end_matches(['\r', '\n']).to_string();
        if let Some(file) = files_by_path.get_mut(&path) {
            let old_lineno = line.old_lineno().filter(|value| *value > 0);
            let new_lineno = line.new_lineno().filter(|value| *value > 0);
            let line_number_old = old_lineno.map(|value| value as u32);
            let line_number_new = new_lineno.map(|value| value as u32);
            file.lines.push(DiffLine {
                line_type: map_line_type(line.origin()),
                content,
                line_number_old,
                line_number_new,
            });
        }
        true
    })
    .map_err(map_git_err)?;

    let mut files = Vec::with_capacity(file_order.len());
    for path in file_order {
        if let Some(file) = files_by_path.remove(&path) {
            files.push(file);
        }
    }

    Ok(DiffPayloadResult { files })
}

pub fn has_uncommitted_changes(repo: &Path) -> Result<bool> {
    let repo = open_repo(repo)?;
    let mut options = StatusOptions::new();
    options
        .include_untracked(true)
        .recurse_untracked_dirs(true)
        .include_ignored(false)
        .include_unmodified(false)
        .exclude_submodules(true);
    let statuses = repo.statuses(Some(&mut options)).map_err(map_git_err)?;
    Ok(statuses
        .iter()
        .any(|entry| entry.status() != Status::CURRENT))
}

#[cfg(test)]
mod tests {
    use super::{
        add_worktree, fast_forward_local_branch_if_behind, get_head_commit, git_merge_branch,
        has_uncommitted_changes,
    };
    use git2::build::CheckoutBuilder;
    use git2::{Repository, Signature};
    use std::fs;
    use std::path::Path;
    use uuid::Uuid;

    #[test]
    fn merge_branch_fast_forwards_target_when_possible() {
        let repo_dir = temp_repo_dir("merge-ff");
        let repo = init_repo(&repo_dir);
        create_commit(&repo, "main.txt", "base\n", "initial");

        checkout_branch(&repo, "feature", true);
        let feature_commit = create_commit(&repo, "feature.txt", "feature\n", "feature");

        checkout_branch(&repo, "main", false);
        git_merge_branch(&repo_dir, "main", "feature").unwrap();

        assert_eq!(repo.head().unwrap().shorthand(), Some("main"));
        assert_eq!(
            get_head_commit(&repo_dir).unwrap(),
            feature_commit.to_string()
        );
        assert!(!has_uncommitted_changes(&repo_dir).unwrap());

        let _ = fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn merge_branch_reports_conflicts_for_manual_resolution() {
        let repo_dir = temp_repo_dir("merge-conflict");
        let repo = init_repo(&repo_dir);
        create_commit(&repo, "shared.txt", "base\n", "initial");

        checkout_branch(&repo, "feature", true);
        create_commit(&repo, "shared.txt", "feature\n", "feature");

        checkout_branch(&repo, "main", false);
        create_commit(&repo, "shared.txt", "main\n", "main");

        let error = git_merge_branch(&repo_dir, "main", "feature").unwrap_err();
        assert!(error
            .to_string()
            .contains("Resolve them manually in the main repository"));

        let _ = fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn fast_forward_checked_out_base_branch_updates_worktree_when_clean() {
        let repo_dir = temp_repo_dir("ff-clean-worktree");
        let repo = init_repo(&repo_dir);
        create_commit(&repo, "main.txt", "base\n", "initial");

        checkout_branch(&repo, "remote-main", true);
        let remote_commit = create_commit(&repo, "main.txt", "remote\n", "remote");
        repo.reference(
            "refs/remotes/origin/main",
            remote_commit,
            true,
            "test remote",
        )
        .unwrap();
        checkout_branch(&repo, "main", false);

        fast_forward_local_branch_if_behind(&repo, &repo_dir, "main", "origin", "main").unwrap();

        assert_eq!(repo.head().unwrap().shorthand(), Some("main"));
        assert_eq!(
            get_head_commit(&repo_dir).unwrap(),
            remote_commit.to_string()
        );
        assert_eq!(
            fs::read_to_string(repo_dir.join("main.txt")).unwrap(),
            "remote\n"
        );
        assert!(!has_uncommitted_changes(&repo_dir).unwrap());

        let _ = fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn fast_forward_checked_out_base_branch_does_not_move_dirty_worktree() {
        let repo_dir = temp_repo_dir("ff-dirty-worktree");
        let repo = init_repo(&repo_dir);
        let local_commit = create_commit(&repo, "main.txt", "base\n", "initial");

        checkout_branch(&repo, "remote-main", true);
        let remote_commit = create_commit(&repo, "main.txt", "remote\n", "remote");
        repo.reference(
            "refs/remotes/origin/main",
            remote_commit,
            true,
            "test remote",
        )
        .unwrap();
        checkout_branch(&repo, "main", false);
        fs::write(repo_dir.join("local.txt"), "local\n").unwrap();

        fast_forward_local_branch_if_behind(&repo, &repo_dir, "main", "origin", "main").unwrap();

        assert_eq!(
            get_head_commit(&repo_dir).unwrap(),
            local_commit.to_string()
        );
        assert_eq!(
            fs::read_to_string(repo_dir.join("main.txt")).unwrap(),
            "base\n"
        );
        assert_eq!(
            fs::read_to_string(repo_dir.join("local.txt")).unwrap(),
            "local\n"
        );
        assert!(has_uncommitted_changes(&repo_dir).unwrap());

        let _ = fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn add_worktree_recovers_from_stale_checked_out_metadata() {
        let repo_dir = temp_repo_dir("stale-worktree-metadata");
        let repo = init_repo(&repo_dir);
        create_commit(&repo, "main.txt", "base\n", "initial");

        let stale_path = temp_repo_dir("stale-worktree-metadata-old");
        add_worktree(
            &repo_dir,
            "feature/stale",
            &stale_path,
            "main",
            "stale-worktree",
        )
        .unwrap();
        fs::remove_dir_all(&stale_path).unwrap();

        let next_path = temp_repo_dir("stale-worktree-metadata-next");
        add_worktree(
            &repo_dir,
            "feature/stale",
            &next_path,
            "main",
            "next-worktree",
        )
        .unwrap();

        let next_repo = Repository::open(&next_path).unwrap();
        assert_eq!(next_repo.head().unwrap().shorthand(), Some("feature/stale"));

        let _ = repo.find_worktree("next-worktree").unwrap().prune(None);
        let _ = fs::remove_dir_all(&next_path);
        let _ = fs::remove_dir_all(&repo_dir);
    }

    fn temp_repo_dir(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("illuc-{label}-{}", Uuid::new_v4()))
    }

    fn init_repo(path: &Path) -> Repository {
        fs::create_dir_all(path).unwrap();
        let repo = Repository::init(path).unwrap();
        let signature = signature();
        let commit_id = {
            let mut index = repo.index().unwrap();
            let tree_id = index.write_tree().unwrap();
            let tree = repo.find_tree(tree_id).unwrap();
            repo.commit(Some("HEAD"), &signature, &signature, "initial", &tree, &[])
                .unwrap()
        };
        {
            let commit = repo.find_commit(commit_id).unwrap();
            repo.branch("main", &commit, true).unwrap();
        }
        repo.set_head("refs/heads/main").unwrap();
        let mut checkout = CheckoutBuilder::new();
        checkout.force();
        repo.checkout_head(Some(&mut checkout)).unwrap();
        repo
    }

    fn checkout_branch(repo: &Repository, branch: &str, create_from_head: bool) {
        if create_from_head {
            let head_commit = repo.head().unwrap().peel_to_commit().unwrap();
            repo.branch(branch, &head_commit, false).unwrap();
        }
        let reference = format!("refs/heads/{branch}");
        let object = repo.revparse_single(reference.as_str()).unwrap();
        let mut checkout = CheckoutBuilder::new();
        checkout.force();
        repo.checkout_tree(&object, Some(&mut checkout)).unwrap();
        repo.set_head(reference.as_str()).unwrap();
    }

    fn create_commit(
        repo: &Repository,
        relative_path: &str,
        content: &str,
        message: &str,
    ) -> git2::Oid {
        let path = repo.workdir().unwrap().join(relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, content).unwrap();

        let mut index = repo.index().unwrap();
        index.add_path(Path::new(relative_path)).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let signature = signature();
        let parent = repo.head().unwrap().peel_to_commit().unwrap();
        repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            message,
            &tree,
            &[&parent],
        )
        .unwrap()
    }

    fn signature() -> Signature<'static> {
        Signature::now("illuc", "illuc@local").unwrap()
    }
}
