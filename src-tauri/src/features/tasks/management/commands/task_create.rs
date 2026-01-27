use crate::commands::CommandResult;
use crate::features::tasks::{TaskManager, TaskSummary};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub base_repo_path: String,
    pub task_title: Option<String>,
    pub base_ref: Option<String>,
    pub branch_name: Option<String>,
}

pub type Response = TaskSummary;

#[tauri::command]
pub async fn task_create(
    manager: tauri::State<'_, TaskManager>,
    app_handle: tauri::AppHandle,
    req: Request,
) -> CommandResult<Response> {
    manager
        .create_task(req, &app_handle)
        .map_err(|err| err.to_string())
}
