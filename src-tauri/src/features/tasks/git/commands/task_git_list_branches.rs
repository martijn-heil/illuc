use crate::commands::CommandResult;
use crate::features::tasks::git;

pub type Request = String;
pub type Response = Vec<String>;

#[tauri::command]
pub async fn task_git_list_branches(path: Request) -> CommandResult<Response> {
    let repo = std::path::PathBuf::from(&path);
    git::list_branches(repo.as_path()).map_err(|err| err.to_string())
}
