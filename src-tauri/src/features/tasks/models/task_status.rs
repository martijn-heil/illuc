use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TaskStatus {
    CreatingWorktree,
    Idle,
    AwaitingApproval,
    Working,
    Completed,
    Failed,
    Stopped,
    Discarded,
}
