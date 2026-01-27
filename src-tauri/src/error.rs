use thiserror::Error;

pub type Result<T> = std::result::Result<T, TaskError>;

#[derive(Debug, Error)]
pub enum TaskError {
    #[error("{0}")]
    Message(String),
    #[error("task not found")]
    NotFound,
    #[error("task is already running")]
    AlreadyRunning,
    #[error("task is not running")]
    NotRunning,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
