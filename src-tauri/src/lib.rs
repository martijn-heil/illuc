mod workflows;

use workflows::{
    handle_select_base_repo, BaseRepoInfo, CreateWorkflowRequest, DiffPayload, DiffRequest,
    DiscardWorkflowRequest, StartWorkflowRequest, StopWorkflowRequest, TerminalResizeRequest,
    TerminalWriteRequest, WorkflowActionRequest, WorkflowManager, WorkflowSummary,
};

type CommandResult<T> = std::result::Result<T, String>;

#[tauri::command]
async fn select_base_repo(path: String) -> CommandResult<BaseRepoInfo> {
    handle_select_base_repo(path).map_err(|err| err.to_string())
}

#[tauri::command]
async fn create_workflow(
    manager: tauri::State<'_, WorkflowManager>,
    app_handle: tauri::AppHandle,
    req: CreateWorkflowRequest,
) -> CommandResult<WorkflowSummary> {
    manager
        .create_workflow(req, &app_handle)
        .map_err(|err| err.to_string())
}

#[tauri::command]
async fn start_workflow(
    manager: tauri::State<'_, WorkflowManager>,
    app_handle: tauri::AppHandle,
    req: StartWorkflowRequest,
) -> CommandResult<WorkflowSummary> {
    manager
        .start_workflow(req, &app_handle)
        .map_err(|err| err.to_string())
}

#[tauri::command]
async fn stop_workflow(
    manager: tauri::State<'_, WorkflowManager>,
    app_handle: tauri::AppHandle,
    req: StopWorkflowRequest,
) -> CommandResult<WorkflowSummary> {
    manager
        .stop_workflow(req, &app_handle)
        .map_err(|err| err.to_string())
}

#[tauri::command]
async fn discard_workflow(
    manager: tauri::State<'_, WorkflowManager>,
    app_handle: tauri::AppHandle,
    req: DiscardWorkflowRequest,
) -> CommandResult<()> {
    manager
        .discard_workflow(req, &app_handle)
        .map_err(|err| err.to_string())
}

#[tauri::command]
async fn terminal_write(
    manager: tauri::State<'_, WorkflowManager>,
    req: TerminalWriteRequest,
) -> CommandResult<()> {
    manager.terminal_write(req).map_err(|err| err.to_string())
}

#[tauri::command]
async fn terminal_resize(
    manager: tauri::State<'_, WorkflowManager>,
    req: TerminalResizeRequest,
) -> CommandResult<()> {
    manager.terminal_resize(req).map_err(|err| err.to_string())
}

#[tauri::command]
async fn get_diff(
    manager: tauri::State<'_, WorkflowManager>,
    req: DiffRequest,
) -> CommandResult<DiffPayload> {
    manager.get_diff(req).map_err(|err| err.to_string())
}

#[tauri::command]
async fn load_existing_worktrees(
    manager: tauri::State<'_, WorkflowManager>,
    app_handle: tauri::AppHandle,
    base_repo_path: String,
) -> CommandResult<Vec<WorkflowSummary>> {
    manager
        .register_existing_worktrees(base_repo_path, &app_handle)
        .map_err(|err| err.to_string())
}

#[tauri::command]
async fn open_worktree_in_vscode(
    manager: tauri::State<'_, WorkflowManager>,
    req: WorkflowActionRequest,
) -> CommandResult<()> {
    manager.open_in_vscode(req).map_err(|err| err.to_string())
}

#[tauri::command]
async fn open_worktree_terminal(
    manager: tauri::State<'_, WorkflowManager>,
    req: WorkflowActionRequest,
) -> CommandResult<()> {
    manager.open_terminal(req).map_err(|err| err.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(WorkflowManager::default())
        .invoke_handler(tauri::generate_handler![
            select_base_repo,
            create_workflow,
            start_workflow,
            stop_workflow,
            discard_workflow,
            terminal_write,
            terminal_resize,
            get_diff,
            load_existing_worktrees,
            open_worktree_in_vscode,
            open_worktree_terminal
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
