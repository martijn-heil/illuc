use crate::error::Result;
use crate::features::tasks::models::BaseRepoInfo;
use crate::features::tasks::git::{run_git, validate_git_repo};
use crate::utils::fs::ensure_directory;
use crate::utils::path::normalize_path_string;
use std::path::PathBuf;

pub fn handle_select_base_repo(path: String) -> Result<BaseRepoInfo> {
    let repo = PathBuf::from(&path);
    ensure_directory(&repo)?;
    validate_git_repo(&repo)?;
    let canonical_path = normalize_path_string(
        &repo.canonicalize().unwrap_or_else(|_| repo.clone()),
    );
    let current_branch = run_git(&repo, ["rev-parse", "--abbrev-ref", "HEAD"])?;
    let head = run_git(&repo, ["rev-parse", "HEAD"])?;
    Ok(BaseRepoInfo {
        path,
        canonical_path,
        current_branch,
        head,
    })
}
