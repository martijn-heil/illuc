import { CommonModule } from "@angular/common";
import { Component, EventEmitter, Input, Output, ViewChild, ElementRef, NgZone } from "@angular/core";
import { FormsModule } from "@angular/forms";
import { AgentKind, TaskSummary, BaseRepoInfo } from "../../task.models";
import { parseTitleParts, TitleParts } from "../../title.utils";
import { TaskTerminalComponent } from "../task-terminal/task-terminal.component";
import { TaskDiffComponent } from "../task-diff/task-diff.component";
import { TaskActionButtonComponent } from "../task-action-button/task-action-button.component";
import { OpenVsCodeButtonComponent } from "../open-vscode-button/open-vscode-button.component";
import { OpenTerminalButtonComponent } from "../open-terminal-button/open-terminal-button.component";
import { StartAgentDropdownComponent } from "../start-agent-dropdown/start-agent-dropdown.component";
import { LoadingButtonComponent } from "../../../shared/components/loading-button/loading-button.component";
import { TaskStore } from "../../task.store";
import { LauncherService } from "../../../launcher/launcher.service";

@Component({
  selector: "app-task-view",
  standalone: true,
  imports: [
    CommonModule,
    FormsModule,
    TaskTerminalComponent,
    TaskDiffComponent,
    TaskActionButtonComponent,
    OpenVsCodeButtonComponent,
    OpenTerminalButtonComponent,
    StartAgentDropdownComponent,
    LoadingButtonComponent,
  ],
  templateUrl: "./task-view.component.html",
  styleUrl: "./task-view.component.css",
})
export class TaskViewComponent {
  @Input() task: TaskSummary | null = null;
  @Input() baseRepo: BaseRepoInfo | null = null;
  @Input() startLoading = false;
  @Input() stopLoading = false;
  @Input() selectRepoLoading = false;
  activePane: "terminal" | "diff" = "terminal";
  isShellTerminalOpen = false;
  isShellResizing = false;
  shellTerminalHeight = 260;
  shellGhostHeight: number | null = null;
  private readonly minShellHeight = 160;
  @ViewChild("shellTerminal") shellTerminal?: TaskTerminalComponent;
  @ViewChild("shellDock") shellDock?: ElementRef<HTMLDivElement>;
  @ViewChild("taskDetail") taskDetail?: ElementRef<HTMLElement>;
  @Output() startTask = new EventEmitter<{ taskId: string; agent: AgentKind }>();
  @Output() stopTask = new EventEmitter<string>();
  @Output() discardTask = new EventEmitter<string>();
  @Output() selectBaseRepo = new EventEmitter<void>();
  showCommitModal = false;
  showPushModal = false;
  commitMessage = "";
  commitStageAll = true;
  commitError = "";
  pushRemote = "origin";
  pushBranch = "";
  pushSetUpstream = true;
  pushError = "";
  isCommitting = false;
  isPushing = false;
  readonly agentKind = AgentKind;

  constructor(
    private readonly taskStore: TaskStore,
    private readonly launcher: LauncherService,
    private readonly zone: NgZone,
  ) {}

  ngOnChanges(): void {
    if (this.task?.taskId) {
      this.isShellTerminalOpen = this.taskStore.isWorktreeTerminalOpen(
        this.task.taskId,
      );
    } else {
      this.isShellTerminalOpen = false;
    }
  }

  statusLabel(): string {
    return this.task?.status.replace(/_/g, " ") ?? "";
  }

  canStart(): boolean {
    return (
      !!this.task &&
      ["STOPPED", "COMPLETED", "FAILED"].includes(this.task.status)
    );
  }

  isRunning(): boolean {
    return !!this.task &&
      ["IDLE", "AWAITING_APPROVAL", "WORKING"].includes(this.task.status);
  }

  titleParts(): TitleParts | null {
    if (!this.task) {
      return null;
    }
    return parseTitleParts(this.task.title);
  }

  startWith(agent: AgentKind): void {
    if (!this.task) {
      return;
    }
    this.startTask.emit({ taskId: this.task.taskId, agent });
  }

  onStop(): void {
    if (this.task) {
      this.stopTask.emit(this.task.taskId);
    }
  }

  onDiscard(): void {
    if (this.task) {
      this.discardTask.emit(this.task.taskId);
    }
  }

  openCommitModal(): void {
    if (!this.task) {
      return;
    }
    this.commitMessage = "";
    this.commitStageAll = true;
    this.commitError = "";
    this.showCommitModal = true;
  }

  closeCommitModal(): void {
    this.showCommitModal = false;
    this.commitMessage = "";
    this.commitError = "";
  }

  async submitCommit(): Promise<void> {
    if (!this.task) {
      return;
    }
    if (this.isCommitting) {
      return;
    }
    if (!this.commitMessage.trim()) {
      this.commitError = "Commit message is required.";
      return;
    }
    this.commitError = "";
    this.isCommitting = true;
    try {
      await this.taskStore.commitTask(
        this.task.taskId,
        this.commitMessage.trim(),
        this.commitStageAll,
      );
      this.closeCommitModal();
    } catch (error: unknown) {
      this.commitError = this.describeError(error, "Unable to commit changes.");
    } finally {
      this.isCommitting = false;
    }
  }

  openPushModal(): void {
    if (!this.task) {
      return;
    }
    this.pushRemote = "origin";
    this.pushBranch = this.task.branchName;
    this.pushSetUpstream = true;
    this.pushError = "";
    this.showPushModal = true;
  }

  closePushModal(): void {
    this.showPushModal = false;
    this.pushError = "";
  }

  async submitPush(): Promise<void> {
    if (!this.task) {
      return;
    }
    if (this.isPushing) {
      return;
    }
    this.pushError = "";
    this.isPushing = true;
    try {
      await this.taskStore.pushTask(
        this.task.taskId,
        this.pushRemote.trim() || "origin",
        this.pushBranch.trim() || this.task.branchName,
        this.pushSetUpstream,
      );
      this.closePushModal();
    } catch (error: unknown) {
      this.pushError = this.describeError(error, "Unable to push changes.");
    } finally {
      this.isPushing = false;
    }
  }

  onSelectBaseRepo(): void {
    this.selectBaseRepo.emit();
  }

  setActivePane(pane: "terminal" | "diff"): void {
    this.activePane = pane;
  }

  toggleShellTerminal(): void {
    this.isShellTerminalOpen = !this.isShellTerminalOpen;
    if (this.task?.taskId) {
      this.taskStore.setWorktreeTerminalOpen(
        this.task.taskId,
        this.isShellTerminalOpen,
      );
    }
  }

  onShellHeaderMouseDown(event: MouseEvent): void {
    if (!this.isShellTerminalOpen) {
      return;
    }
    this.startShellResize(event);
  }

  onShellHeaderClick(): void {
    if (!this.isShellTerminalOpen) {
      this.toggleShellTerminal();
    }
  }

  startShellResize(event: MouseEvent): void {
    if (!this.isShellTerminalOpen) {
      return;
    }
    event.preventDefault();
    this.isShellResizing = true;
    const startY = event.clientY;
    const startHeight = this.shellTerminalHeight;
    let latestHeight = startHeight;
    let rafId: number | null = null;
    const dockEl = this.shellDock?.nativeElement;
    const containerHeight = this.taskDetail?.nativeElement.clientHeight ?? window.innerHeight;
    const maxShellHeight = Math.max(this.minShellHeight, containerHeight - 16);

    const handleMove = (moveEvent: MouseEvent) => {
      const delta = startY - moveEvent.clientY;
      const next = Math.max(
        this.minShellHeight,
        Math.min(maxShellHeight, startHeight + delta),
      );
      latestHeight = next;
      if (rafId === null) {
        rafId = requestAnimationFrame(() => {
          if (dockEl) {
            dockEl.style.height = `${latestHeight}px`;
          } else {
            this.shellTerminalHeight = latestHeight;
          }
          rafId = null;
        });
      }
    };

    const handleUp = () => {
      window.removeEventListener("mousemove", handleMove);
      window.removeEventListener("mouseup", handleUp);
      if (rafId !== null) {
        cancelAnimationFrame(rafId);
        rafId = null;
      }
      this.zone.run(() => {
        this.shellTerminalHeight = latestHeight;
        this.isShellResizing = false;
        this.shellTerminal?.forceBackendResizeNow(true);
      });
    };

    this.zone.runOutsideAngular(() => {
      window.addEventListener("mousemove", handleMove);
      window.addEventListener("mouseup", handleUp);
    });
  }



  async openInExplorer(event: Event, path: string): Promise<void> {
    event.preventDefault();
    try {
      await this.launcher.openInExplorer(path);
    } catch (error) {
      console.error("Failed to open explorer", error);
    }
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
}
