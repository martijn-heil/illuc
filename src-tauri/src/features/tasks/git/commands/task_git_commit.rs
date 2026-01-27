use crate::commands::CommandResult;
use crate::features::tasks::TaskManager;
use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub task_id: Uuid,
    pub message: String,
    pub stage_all: Option<bool>,
}

pub type Response = ();

#[tauri::command]
pub async fn task_git_commit(
    manager: tauri::State<'_, TaskManager>,
    req: Request,
) -> CommandResult<Response> {
    manager.commit_task(req).map_err(|err| err.to_string())
}
