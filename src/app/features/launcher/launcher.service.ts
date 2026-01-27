import { Injectable } from "@angular/core";
import { invoke } from "@tauri-apps/api/core";

@Injectable({
    providedIn: "root",
})
export class LauncherService {
    async openInVsCode(path: string): Promise<void> {
        await invoke("open_path_in_vscode", { path });
    }

    async openTerminal(path: string): Promise<void> {
        await invoke("open_path_terminal", { path });
    }

    async openInExplorer(path: string): Promise<void> {
        await invoke("open_path_in_explorer", { path });
    }
}
