use crate::commands::CommandResult;
use crate::features::tasks::{AgentKind, TaskManager, TaskSummary};
use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub task_id: Uuid,
    pub cols: Option<u16>,
    pub rows: Option<u16>,
    pub agent: Option<AgentKind>,
}

pub type Response = TaskSummary;

#[tauri::command]
pub async fn task_start(
    manager: tauri::State<'_, TaskManager>,
    app_handle: tauri::AppHandle,
    req: Request,
) -> CommandResult<Response> {
    manager
        .start_task(req, &app_handle)
        .map_err(|err| err.to_string())
}
