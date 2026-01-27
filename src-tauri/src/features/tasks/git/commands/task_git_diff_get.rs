use crate::commands::CommandResult;
use crate::features::tasks::{DiffPayload, TaskManager};
use crate::features::tasks::git::DiffMode;
use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub task_id: Uuid,
    pub ignore_whitespace: Option<bool>,
    pub mode: Option<DiffMode>,
}

pub type Response = DiffPayload;

#[tauri::command]
pub async fn task_git_diff_get(
    manager: tauri::State<'_, TaskManager>,
    req: Request,
) -> CommandResult<Response> {
    manager.get_diff(req).map_err(|err| err.to_string())
}
