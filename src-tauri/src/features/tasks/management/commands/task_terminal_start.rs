use crate::commands::CommandResult;
use crate::features::tasks::{TaskManager, TerminalKind};
use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub task_id: Uuid,
    pub kind: TerminalKind,
    pub cols: Option<u16>,
    pub rows: Option<u16>,
}

pub type Response = ();

#[tauri::command]
pub async fn task_terminal_start(
    manager: tauri::State<'_, TaskManager>,
    app_handle: tauri::AppHandle,
    req: Request,
) -> CommandResult<Response> {
    match req.kind {
        TerminalKind::Agent => Ok(()),
        TerminalKind::Worktree => manager
            .start_worktree_terminal(req, &app_handle)
            .map_err(|err| err.to_string()),
    }
}
