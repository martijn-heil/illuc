import { CommonModule } from "@angular/common";
import { Component, EventEmitter, Input, Output } from "@angular/core";
import { TaskStatus, TaskSummary, BaseRepoInfo } from "../../../task.models";
import { parseTitleParts, TitleParts } from "../../../title.utils";
import { TaskActionButtonComponent } from "../../../actions/components/task-action-button/task-action-button.component";
import { OpenVsCodeButtonComponent } from "../../../workspace/components/open-vscode-button/open-vscode-button.component";
import { OpenTerminalButtonComponent } from "../../../workspace/components/open-terminal-button/open-terminal-button.component";
import { LauncherService } from "../../../../launcher/launcher.service";

@Component({
    selector: "app-task-sidebar",
    standalone: true,
    imports: [
        CommonModule,
        TaskActionButtonComponent,
        OpenVsCodeButtonComponent,
        OpenTerminalButtonComponent,
    ],
    templateUrl: "./task-sidebar.component.html",
    styleUrl: "./task-sidebar.component.css",
})
export class TaskSidebarComponent {
    @Input({ required: true }) tasks: TaskSummary[] | null = [];
    @Input() selectedTaskId: string | null = null;
    @Input() baseRepo: BaseRepoInfo | null = null;
    @Input() stopLoadingIds: Set<string> = new Set();
    @Output() selectTask = new EventEmitter<string>();
    @Output() stopTask = new EventEmitter<string>();
    @Output() discardTask = new EventEmitter<string>();
    @Output() createTask = new EventEmitter<void>();

    constructor(private readonly launcher: LauncherService) {}

    trackById(_: number, task: TaskSummary): string {
        return task.taskId;
    }

    onSelect(taskId: string): void {
        this.selectTask.emit(taskId);
    }

    onStop(taskId: string): void {
        this.stopTask.emit(taskId);
    }

    onDiscard(taskId: string): void {
        this.discardTask.emit(taskId);
    }

    statusLabel(status: TaskStatus): string {
        return status.replace(/_/g, " ");
    }

    isRunning(status: TaskStatus): boolean {
        return (
            status === "IDLE" ||
            status === "AWAITING_APPROVAL" ||
            status === "WORKING"
        );
    }

    titleParts(title: string): TitleParts {
        return parseTitleParts(title);
    }

    async openInExplorer(event: Event, path: string): Promise<void> {
        event.preventDefault();
        try {
            await this.launcher.openInExplorer(path);
        } catch (error) {
            console.error("Failed to open explorer", error);
        }
    }
}
