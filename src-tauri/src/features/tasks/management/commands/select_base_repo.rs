use crate::commands::CommandResult;
use crate::features::tasks::{handle_select_base_repo, BaseRepoInfo};

pub type Request = String;
pub type Response = BaseRepoInfo;

#[tauri::command]
pub async fn select_base_repo(path: Request) -> CommandResult<Response> {
    handle_select_base_repo(path).map_err(|err| err.to_string())
}
