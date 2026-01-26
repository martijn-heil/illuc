use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartWorktreeTerminalRequest {
    pub task_id: Uuid,
    pub cols: Option<u16>,
    pub rows: Option<u16>,
}
