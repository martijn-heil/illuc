use crate::commands::CommandResult;
use crate::features::launcher;

pub type Request = String;
pub type Response = ();

#[tauri::command]
pub async fn open_path_in_explorer(path: Request) -> CommandResult<Response> {
    let target = std::path::PathBuf::from(path);
    launcher::open_path_in_explorer(target.as_path()).map_err(|err| err.to_string())
}
