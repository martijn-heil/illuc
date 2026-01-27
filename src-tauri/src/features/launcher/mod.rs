use crate::error::Result;
use crate::utils::fs::ensure_directory;
use std::path::Path;

mod terminal;
mod explorer;
mod vscode;
pub mod commands;

pub fn open_path_in_vscode(path: &Path) -> Result<()> {
    ensure_directory(path)?;
    vscode::spawn(path)
}

pub fn open_path_terminal(path: &Path) -> Result<()> {
    ensure_directory(path)?;
    terminal::spawn(path)
}

pub fn open_path_in_explorer(path: &Path) -> Result<()> {
    ensure_directory(path)?;
    explorer::spawn(path)
}
