use crate::error::{Result, TaskError};
use std::path::Path;
use std::process::Command;

pub fn spawn(path: &Path) -> Result<()> {
    #[cfg(windows)]
    let candidates = ["code.cmd", "code.exe", "code"];
    #[cfg(not(windows))]
    let candidates = ["code"];

    for candidate in candidates {
        let result = Command::new(candidate).arg(path).spawn();
        match result {
            Ok(_) => return Ok(()),
            Err(err) => {
                if err.kind() == std::io::ErrorKind::NotFound {
                    continue;
                } else {
                    return Err(err.into());
                }
            }
        }
    }
    Err(TaskError::Message(
        "Unable to launch VS Code. Make sure the `code` command is available.".to_string(),
    ))
}
