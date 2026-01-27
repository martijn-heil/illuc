use crate::features::tasks::models::task_status::TaskStatus;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskSummary {
    pub task_id: Uuid,
    pub title: String,
    pub status: TaskStatus,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub ended_at: Option<DateTime<Utc>>,
    pub worktree_path: String,
    pub branch_name: String,
    pub base_branch: String,
    pub base_repo_path: String,
    pub base_commit: String,
    pub exit_code: Option<i32>,
}
