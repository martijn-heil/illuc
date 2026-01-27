import { Injectable } from "@angular/core";
import { invoke } from "@tauri-apps/api/core";

@Injectable({
    providedIn: "root",
})
export class TaskGitService {
    async listBranches(baseRepoPath: string): Promise<string[]> {
        return invoke<string[]>("task_git_list_branches", {
            path: baseRepoPath,
        });
    }
}
