use crate::commands::CommandResult;
use crate::features::tasks::{TaskManager, TerminalKind};
use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub task_id: Uuid,
    pub kind: TerminalKind,
    pub cols: u16,
    pub rows: u16,
}

pub type Response = ();

#[tauri::command]
pub async fn task_terminal_resize(
    manager: tauri::State<'_, TaskManager>,
    req: Request,
) -> CommandResult<Response> {
    match req.kind {
        TerminalKind::Agent => manager.terminal_resize(req).map_err(|err| err.to_string()),
        TerminalKind::Worktree => manager
            .worktree_terminal_resize(req)
            .map_err(|err| err.to_string()),
    }
}
