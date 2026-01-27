mod commands;
mod error;
mod features;
mod utils;

use crate::features::launcher::commands::open_path_in_explorer::open_path_in_explorer;
use crate::features::launcher::commands::open_path_in_vscode::open_path_in_vscode;
use crate::features::launcher::commands::open_path_terminal::open_path_terminal;
use crate::features::tasks::git::commands::task_git_commit::task_git_commit;
use crate::features::tasks::git::commands::task_git_diff_get::task_git_diff_get;
use crate::features::tasks::git::commands::task_git_diff_watch_start::task_git_diff_watch_start;
use crate::features::tasks::git::commands::task_git_diff_watch_stop::task_git_diff_watch_stop;
use crate::features::tasks::git::commands::task_git_list_branches::task_git_list_branches;
use crate::features::tasks::git::commands::task_git_push::task_git_push;
use crate::features::tasks::management::commands::select_base_repo::select_base_repo;
use crate::features::tasks::management::commands::task_create::task_create;
use crate::features::tasks::management::commands::task_discard::task_discard;
use crate::features::tasks::management::commands::task_load_existing::task_load_existing;
use crate::features::tasks::management::commands::task_open_worktree_in_vscode::task_open_worktree_in_vscode;
use crate::features::tasks::management::commands::task_open_worktree_terminal::task_open_worktree_terminal;
use crate::features::tasks::management::commands::task_start::task_start;
use crate::features::tasks::management::commands::task_stop::task_stop;
use crate::features::tasks::management::commands::task_terminal_start::task_terminal_start;
use crate::features::tasks::management::commands::task_terminal_resize::task_terminal_resize;
use crate::features::tasks::management::commands::task_terminal_write::task_terminal_write;
use crate::features::tasks::TaskManager;
use log::info;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    dotenvy::dotenv().ok();
    let _ = env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("illuc=debug,tauri=info"),
    )
    .format_timestamp_millis()
    .try_init();
    info!("starting illuc tauri app");

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(TaskManager::default())
        .invoke_handler(tauri::generate_handler![
            select_base_repo,
            task_create,
            task_start,
            task_stop,
            task_discard,
            task_terminal_write,
            task_terminal_resize,
            task_terminal_start,
            task_git_diff_get,
            task_git_diff_watch_start,
            task_git_diff_watch_stop,
            task_git_commit,
            task_git_push,
            task_load_existing,
            task_open_worktree_in_vscode,
            task_open_worktree_terminal,
            open_path_in_vscode,
            open_path_terminal,
            open_path_in_explorer,
            task_git_list_branches
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
