use crate::commands::CommandResult;
use crate::features::tasks::{TaskManager, TaskSummary};
use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub task_id: Uuid,
}

pub type Response = TaskSummary;

#[tauri::command]
pub async fn task_stop(
    manager: tauri::State<'_, TaskManager>,
    app_handle: tauri::AppHandle,
    req: Request,
) -> CommandResult<Response> {
    manager.stop_task(req, &app_handle).map_err(|err| err.to_string())
}
