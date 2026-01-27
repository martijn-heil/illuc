import { CommonModule } from "@angular/common";
import { Component, OnDestroy, OnInit } from "@angular/core";
import { FormsModule } from "@angular/forms";
import { open } from "@tauri-apps/plugin-dialog";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { TaskSidebarComponent } from "../../../tasks/sidebar/components/task-sidebar/task-sidebar.component";
import { TaskViewComponent } from "../../../tasks/view/components/task-view/task-view.component";
import { AgentKind } from "../../../tasks/task.models";
import { deriveTitleFromBranch } from "../../../tasks/title.utils";
import { TaskStore } from "../../../tasks/task.store";
import { LauncherService } from "../../../launcher/launcher.service";
import { LoadingButtonComponent } from "../../../../shared/components/loading-button/loading-button.component";

@Component({
    selector: "app-root",
    standalone: true,
    imports: [
        CommonModule,
        FormsModule,
        TaskSidebarComponent,
        TaskViewComponent,
        LoadingButtonComponent,
    ],
    templateUrl: "./app.component.html",
    styleUrl: "./app.component.css",
})
export class AppComponent implements OnInit, OnDestroy {
    showCreateModal = false;
    branchNameInput = "";
    branchNameError = "";
    confirmDiscardTaskId: string | null = null;
    confirmDiscardTitle = "";
    confirmDiscardBranch = "";
    confirmDiscardError = "";
    baseBranchSelection = "";
    private readonly appWindow = getCurrentWindow();
    isMaximized = false;
    private unlistenResize?: () => void;
    isSelectingRepo = false;
    isCreatingTask = false;
    isDiscardingTask = false;
    private readonly startingTaskIds = new Set<string>();
    private readonly stoppingTaskIds = new Set<string>();

    constructor(
        public readonly taskStore: TaskStore,
        private readonly launcher: LauncherService,
    ) {}

    ngOnInit(): void {
        void this.refreshMaximizeState();
        void this.appWindow
            .onResized(() => {
                void this.refreshMaximizeState();
            })
            .then((unlisten) => {
                this.unlistenResize = unlisten;
            });
    }

    ngOnDestroy(): void {
        this.unlistenResize?.();
        this.unlistenResize = undefined;
    }

    tasks() {
        return this.taskStore.tasks();
    }

    selectedTask() {
        return this.taskStore.selectedTask();
    }

    baseRepo() {
        return this.taskStore.baseRepo();
    }

    branchOptions() {
        return this.taskStore.branches();
    }

    async browseForRepo(): Promise<void> {
        if (this.isSelectingRepo) {
            return;
        }
        this.isSelectingRepo = true;
        try {
            const selection = await open({
                directory: true,
                multiple: false,
                title: "Select base repository",
            });
            if (typeof selection === "string") {
                await this.loadBaseRepo(selection);
            }
        } finally {
            this.isSelectingRepo = false;
        }
    }

    private async loadBaseRepo(path: string): Promise<void> {
        try {
            await this.taskStore.selectBaseRepo(path);
        } catch (error: unknown) {
            console.error(
                this.describeError(
                    error,
                    "Unable to open the selected repository.",
                ),
            );
        }
    }

    openCreateTaskModal(): void {
        if (!this.baseRepo()) {
            console.error("Select a base repository before creating tasks.");
            return;
        }
        this.branchNameInput = "";
        this.branchNameError = "";
        this.baseBranchSelection = this.taskStore.defaultBaseBranch() ?? "";
        this.showCreateModal = true;
    }

    closeCreateTaskModal(): void {
        this.showCreateModal = false;
        this.branchNameInput = "";
        this.branchNameError = "";
        this.baseBranchSelection = "";
    }

    async submitNewTask(): Promise<void> {
        const branch = this.branchNameInput.trim();
        if (!branch) {
            this.branchNameError = "Branch name is required.";
            return;
        }
        if (!this.baseRepo()) {
            this.branchNameError = "Select a base repository first.";
            return;
        }
        if (this.isCreatingTask) {
            return;
        }
        this.branchNameError = "";
        const title = deriveTitleFromBranch(branch);
        this.isCreatingTask = true;
        try {
            await this.taskStore.createTask(
                branch,
                title,
                this.baseBranchSelection,
            );
            this.closeCreateTaskModal();
        } catch (error: unknown) {
            this.branchNameError = this.describeError(
                error,
                "Unable to create task.",
            );
        } finally {
            this.isCreatingTask = false;
        }
    }

    async startTask(payload: {
        taskId: string;
        agent: AgentKind;
    }): Promise<void> {
        if (this.startingTaskIds.has(payload.taskId)) {
            return;
        }
        this.startingTaskIds.add(payload.taskId);
        try {
            await this.taskStore.startTask(payload.taskId, payload.agent);
        } catch (error: unknown) {
            console.error(this.describeError(error, "Unable to start task."));
        } finally {
            this.startingTaskIds.delete(payload.taskId);
        }
    }

    async stopTask(taskId: string): Promise<void> {
        if (this.stoppingTaskIds.has(taskId)) {
            return;
        }
        this.stoppingTaskIds.add(taskId);
        try {
            await this.taskStore.stopTask(taskId);
        } catch (error: unknown) {
            console.error(this.describeError(error, "Unable to stop task."));
        } finally {
            this.stoppingTaskIds.delete(taskId);
        }
    }

    discardTask(taskId: string): void {
        const task =
            this.taskStore.tasks().find((wf) => wf.taskId === taskId) ?? null;
        this.confirmDiscardTaskId = taskId;
        this.confirmDiscardTitle = task?.title ?? "Selected task";
        this.confirmDiscardBranch = task?.branchName ?? "";
        this.confirmDiscardError = "";
    }

    selectTask(taskId: string): void {
        this.taskStore.selectTask(taskId);
    }

    cancelDiscardTask(): void {
        this.confirmDiscardTaskId = null;
        this.confirmDiscardTitle = "";
        this.confirmDiscardBranch = "";
        this.confirmDiscardError = "";
    }

    async confirmDiscardTask(): Promise<void> {
        if (!this.confirmDiscardTaskId) {
            return;
        }
        if (this.isDiscardingTask) {
            return;
        }
        this.isDiscardingTask = true;
        try {
            await this.taskStore.discardTask(this.confirmDiscardTaskId);
            this.cancelDiscardTask();
        } catch (error: unknown) {
            this.confirmDiscardError = this.describeError(
                error,
                "Unable to discard task.",
            );
        } finally {
            this.isDiscardingTask = false;
        }
    }

    async minimizeWindow(): Promise<void> {
        await this.appWindow.minimize();
    }

    async toggleMaximizeWindow(): Promise<void> {
        await this.appWindow.toggleMaximize();
        await this.refreshMaximizeState();
    }

    async closeWindow(): Promise<void> {
        await this.appWindow.close();
    }

    async openInExplorer(event: Event, path: string): Promise<void> {
        event.preventDefault();
        try {
            await this.launcher.openInExplorer(path);
        } catch (error) {
            console.error("Failed to open explorer", error);
        }
    }

    private async refreshMaximizeState(): Promise<void> {
        this.isMaximized = await this.appWindow.isMaximized();
    }

    private describeError(error: unknown, fallback: string): string {
        if (typeof error === "string") {
            return error;
        }
        if (error && typeof error === "object" && "message" in error) {
            return String((error as { message: string }).message);
        }
        return fallback;
    }

    isStartingTask(taskId: string | null | undefined): boolean {
        return !!taskId && this.startingTaskIds.has(taskId);
    }

    isStoppingTask(taskId: string | null | undefined): boolean {
        return !!taskId && this.stoppingTaskIds.has(taskId);
    }

    stoppingTasks(): Set<string> {
        return this.stoppingTaskIds;
    }
}
