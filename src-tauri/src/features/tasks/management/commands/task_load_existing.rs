use crate::commands::CommandResult;
use crate::features::tasks::{TaskManager, TaskSummary};

pub type Request = String;
pub type Response = Vec<TaskSummary>;

#[tauri::command]
pub async fn task_load_existing(
    manager: tauri::State<'_, TaskManager>,
    app_handle: tauri::AppHandle,
    base_repo_path: Request,
) -> CommandResult<Response> {
    manager
        .register_existing_worktrees(base_repo_path, &app_handle)
        .map_err(|err| err.to_string())
}
