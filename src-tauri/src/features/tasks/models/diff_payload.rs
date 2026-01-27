use crate::features::tasks::git::DiffFile;
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffPayload {
    pub task_id: Uuid,
    pub files: Vec<DiffFile>,
    pub unified_diff: String,
}
