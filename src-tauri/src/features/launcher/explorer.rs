use crate::error::{Result, TaskError};
use std::path::Path;
use std::process::Command;

#[cfg(target_os = "windows")]
pub fn spawn(path: &Path) -> Result<()> {
    Command::new("explorer")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|err| TaskError::Message(format!("Failed to open explorer: {err}")))
}

#[cfg(target_os = "macos")]
pub fn spawn(path: &Path) -> Result<()> {
    Command::new("open")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|err| TaskError::Message(format!("Failed to open Finder: {err}")))
}

#[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
pub fn spawn(path: &Path) -> Result<()> {
    Command::new("xdg-open")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|err| TaskError::Message(format!("Failed to open file browser: {err}")))
}
