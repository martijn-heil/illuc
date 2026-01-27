use crate::error::{Result, TaskError};
use std::path::Path;
use std::process::Command;

pub fn spawn(path: &Path) -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        let path_str = path.to_string_lossy().to_string();
        let attempt_cmd = |mut command: Command| -> Result<bool> {
            match command.spawn() {
                Ok(_) => Ok(true),
                Err(err) => {
                    if err.kind() == std::io::ErrorKind::NotFound {
                        Ok(false)
                    } else {
                        Err(err.into())
                    }
                }
            }
        };

        if attempt_cmd({
            let mut cmd = Command::new("wt");
            cmd.args(["-d", &path_str]);
            cmd
        })? {
            return Ok(());
        }

        for candidate in ["alacritty", "alacritty.exe"] {
            if attempt_cmd({
                let mut cmd = Command::new(candidate);
                cmd.args(["--working-directory", &path_str]);
                cmd
            })? {
                return Ok(());
            }
        }

        if attempt_cmd({
            let mut cmd = Command::new("cmd");
            cmd.args([
                "/C",
                "start",
                "cmd",
                "/K",
                &format!("cd /d \"{}\"", path_str),
            ]);
            cmd
        })? {
            return Ok(());
        }

        if attempt_cmd({
            let mut cmd = Command::new("cmd");
            cmd.args([
                "/C",
                "start",
                "powershell",
                "-NoExit",
                "-Command",
                &format!("Set-Location -Path \"{}\"", path_str),
            ]);
            cmd
        })? {
            return Ok(());
        }

        Err(TaskError::Message(
            "Unable to launch a terminal window. Install Windows Terminal or ensure cmd.exe is available."
                .to_string(),
        ))
    }
    #[cfg(not(target_os = "windows"))]
    {
        let path_str = path.to_string_lossy().to_string();
        let attempts: Vec<(&str, Vec<&str>)> = vec![
            (
                "x-terminal-emulator",
                vec!["--working-directory", path_str.as_str()],
            ),
            (
                "gnome-terminal",
                vec!["--working-directory", path_str.as_str()],
            ),
            ("konsole", vec!["--workdir", path_str.as_str()]),
            (
                "xfce4-terminal",
                vec!["--working-directory", path_str.as_str()],
            ),
            ("kitty", vec!["--directory", path_str.as_str()]),
            ("alacritty", vec!["--working-directory", path_str.as_str()]),
            ("terminator", vec!["--working-directory", path_str.as_str()]),
            ("tilix", vec!["--working-directory", path_str.as_str()]),
        ];
        for (bin, args) in attempts {
            let result = Command::new(bin).args(args).spawn();
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
            "Unable to find a supported terminal application. Install gnome-terminal, kitty, or another supported terminal."
                .to_string(),
        ))
    }
}
