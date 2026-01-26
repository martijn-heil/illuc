import { Injectable, NgZone, computed, signal } from "@angular/core";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { Observable, Subject } from "rxjs";
import {
  AgentKind,
  BaseRepoInfo,
  DiffMode,
  DiffPayload,
  TerminalExitEvent,
  TerminalOutputEvent,
  TaskSummary,
  WorktreeTerminalExitEvent,
  WorktreeTerminalOutputEvent,
} from "./task.models";
import { TaskGitService } from "./git/task-git.service";

@Injectable({
  providedIn: "root",
})
export class TaskStore {
  private readonly tasksSignal = signal<TaskSummary[]>([]);
  private readonly baseRepoSignal = signal<BaseRepoInfo | null>(null);
  private readonly selectedTaskIdSignal = signal<string | null>(null);
  private readonly branchOptionsSignal = signal<string[]>([]);
  private readonly terminalBuffers = new Map<string, string>();
  private readonly terminalStreams = new Map<string, Subject<string>>();
  private readonly terminalSizes = new Map<string, { cols: number; rows: number }>();
  private lastTerminalSize: { cols: number; rows: number } | null = null;
  private readonly worktreeTerminalBuffers = new Map<string, string>();
  private readonly worktreeTerminalStreams = new Map<string, Subject<string>>();
  private readonly worktreeTerminalSizes = new Map<string, { cols: number; rows: number }>();
  private lastWorktreeTerminalSize: { cols: number; rows: number } | null = null;
  private readonly worktreeTerminalOpenState = new Map<string, boolean>();
  private readonly unlistenFns: UnlistenFn[] = [];

  readonly tasks = this.tasksSignal.asReadonly();
  readonly baseRepo = this.baseRepoSignal.asReadonly();
  readonly selectedTaskId = this.selectedTaskIdSignal.asReadonly();
  readonly branchOptions = this.branchOptionsSignal.asReadonly();
  readonly selectedTask = computed(() => {
    const id = this.selectedTaskIdSignal();
    if (!id) {
      return null;
    }
    return this.tasksSignal().find((wf) => wf.taskId === id) ?? null;
  });

  constructor(
    private readonly zone: NgZone,
    private readonly taskGit: TaskGitService,
  ) {
    this.registerEventListeners();
    window.addEventListener("unload", () => this.teardown());
  }

  async selectBaseRepo(path: string): Promise<BaseRepoInfo> {
    const repo = await invoke<BaseRepoInfo>("select_base_repo", { path });
    const normalized: BaseRepoInfo = {
      ...repo,
      path: repo.canonicalPath,
    };
    this.baseRepoSignal.set(normalized);
    this.tasksSignal.set([]);
    this.selectedTaskIdSignal.set(null);
    this.branchOptionsSignal.set([]);
    this.terminalBuffers.clear();
    this.terminalStreams.clear();
    this.worktreeTerminalBuffers.clear();
    this.worktreeTerminalStreams.clear();
    this.worktreeTerminalOpenState.clear();
    await this.loadExistingTasks(normalized.path);
    await this.loadBranches(normalized.path);
    return normalized;
  }

  async createTask(
    branchName: string,
    displayTitle: string,
    baseBranch?: string | null,
  ): Promise<TaskSummary> {
    const repo = this.baseRepoSignal();
    if (!repo) {
      throw new Error("Select a base repository before creating tasks.");
    }
    const baseRef = baseBranch?.trim() || repo.currentBranch || repo.head;
    const summary = await invoke<TaskSummary>("create_task", {
      req: {
        baseRepoPath: repo.path,
        baseRef,
        taskTitle: displayTitle.trim() || undefined,
        branchName: branchName.trim(),
      },
    });
    this.upsertTask(summary);
    this.selectedTaskIdSignal.set(summary.taskId);
    return summary;
  }

  async startTask(taskId: string, agent?: AgentKind): Promise<TaskSummary> {
    const size = this.terminalSizes.get(taskId) ?? this.lastTerminalSize;
    const summary = await invoke<TaskSummary>("start_task", {
      req: {
        taskId,
        cols: size?.cols,
        rows: size?.rows,
        agent,
      },
    });
    this.upsertTask(summary);
    return summary;
  }

  async stopTask(taskId: string): Promise<TaskSummary> {
    const summary = await invoke<TaskSummary>("stop_task", {
      req: { taskId },
    });
    this.upsertTask(summary);
    return summary;
  }

  async discardTask(taskId: string): Promise<void> {
    await invoke("discard_task", { req: { taskId } });
    this.removeTask(taskId);
  }

  async writeToTask(taskId: string, data: string): Promise<void> {
    await invoke("terminal_write", { req: { taskId, data } });
  }

  async resizeTaskTerminal(
    taskId: string,
    cols: number,
    rows: number,
  ): Promise<void> {
    await invoke("terminal_resize", {
      req: {
        taskId,
        cols,
        rows,
      },
    });
  }

  async startWorktreeTerminal(taskId: string): Promise<void> {
    const size =
      this.worktreeTerminalSizes.get(taskId) ?? this.lastWorktreeTerminalSize;
    await invoke("start_worktree_terminal", {
      req: {
        taskId,
        cols: size?.cols,
        rows: size?.rows,
      },
    });
  }

  async writeToWorktreeTerminal(taskId: string, data: string): Promise<void> {
    await invoke("worktree_terminal_write", { req: { taskId, data } });
  }

  async resizeWorktreeTerminal(
    taskId: string,
    cols: number,
    rows: number,
  ): Promise<void> {
    await invoke("worktree_terminal_resize", {
      req: {
        taskId,
        cols,
        rows,
      },
    });
  }

  async getDiff(
    taskId: string,
    ignoreWhitespace = false,
    mode: DiffMode = "worktree",
  ): Promise<DiffPayload> {
    return invoke<DiffPayload>("get_diff", {
      req: {
        taskId,
        ignoreWhitespace,
        mode,
      },
    });
  }

  async startDiffWatch(taskId: string): Promise<void> {
    await invoke("start_diff_watch", { req: { taskId } });
  }

  async stopDiffWatch(taskId: string): Promise<void> {
    await invoke("stop_diff_watch", { req: { taskId } });
  }

  async commitTask(
    taskId: string,
    message: string,
    stageAll = true,
  ): Promise<void> {
    await invoke("commit_task", {
      req: {
        taskId,
        message,
        stageAll,
      },
    });
  }

  async pushTask(
    taskId: string,
    remote = "origin",
    branch?: string,
    setUpstream = true,
  ): Promise<void> {
    await invoke("push_task", {
      req: {
        taskId,
        remote,
        branch,
        setUpstream,
      },
    });
  }

  selectTask(taskId: string | null): void {
    this.selectedTaskIdSignal.set(taskId);
  }

  branches(): string[] {
    return this.branchOptionsSignal();
  }

  defaultBaseBranch(): string | null {
    return this.baseRepoSignal()?.currentBranch ?? null;
  }

  getTerminalBuffer(taskId: string): string {
    return this.terminalBuffers.get(taskId) ?? "";
  }

  terminalOutput$(taskId: string): Observable<string> {
    const stream = this.ensureTerminalStream(taskId);
    return stream.asObservable();
  }

  getWorktreeTerminalBuffer(taskId: string): string {
    return this.worktreeTerminalBuffers.get(taskId) ?? "";
  }

  worktreeTerminalOutput$(taskId: string): Observable<string> {
    const stream = this.ensureWorktreeTerminalStream(taskId);
    return stream.asObservable();
  }

  clearTerminal(taskId: string): void {
    this.terminalBuffers.set(taskId, "");
    const stream = this.ensureTerminalStream(taskId);
    stream.next("\u001bc");
  }

  clearWorktreeTerminal(taskId: string): void {
    this.worktreeTerminalBuffers.set(taskId, "");
    const stream = this.ensureWorktreeTerminalStream(taskId);
    stream.next("\u001bc");
  }

  recordTerminalSize(taskId: string, cols: number, rows: number): void {
    if (cols <= 0 || rows <= 0) {
      return;
    }
    this.lastTerminalSize = { cols, rows };
    if (taskId) {
      this.terminalSizes.set(taskId, { cols, rows });
    }
  }

  recordWorktreeTerminalSize(taskId: string, cols: number, rows: number): void {
    if (cols <= 0 || rows <= 0) {
      return;
    }
    this.lastWorktreeTerminalSize = { cols, rows };
    if (taskId) {
      this.worktreeTerminalSizes.set(taskId, { cols, rows });
    }
  }

  isWorktreeTerminalOpen(taskId: string): boolean {
    return this.worktreeTerminalOpenState.get(taskId) ?? false;
  }

  setWorktreeTerminalOpen(taskId: string, isOpen: boolean): void {
    if (!taskId) {
      return;
    }
    this.worktreeTerminalOpenState.set(taskId, isOpen);
  }

  private registerEventListeners(): void {
    void listen<TaskSummary>("task_status_changed", (event) => {
      this.zone.run(() => {
        this.upsertTask(event.payload);
      });
    }).then((unlisten) => this.unlistenFns.push(unlisten));

    void listen<TerminalOutputEvent>("task_terminal_output", (event) => {
      this.zone.run(() => {
        this.pushTerminalOutput(event.payload.taskId, event.payload.data);
      });
    }).then((unlisten) => this.unlistenFns.push(unlisten));

    void listen<TerminalExitEvent>("task_terminal_exit", (event) => {
      this.zone.run(() => {
        console.info(
          `Task ${event.payload.taskId} exited with code ${event.payload.exitCode}`,
        );
      });
    }).then((unlisten) => this.unlistenFns.push(unlisten));

    void listen<WorktreeTerminalOutputEvent>(
      "worktree_terminal_output",
      (event) => {
        this.zone.run(() => {
          this.pushWorktreeTerminalOutput(event.payload.taskId, event.payload.data);
        });
      },
    ).then((unlisten) => this.unlistenFns.push(unlisten));

    void listen<WorktreeTerminalExitEvent>(
      "worktree_terminal_exit",
      (event) => {
        this.zone.run(() => {
          console.info(
            `Worktree terminal ${event.payload.taskId} exited with code ${event.payload.exitCode}`,
          );
        });
      },
    ).then((unlisten) => this.unlistenFns.push(unlisten));
  }

  private upsertTask(summary: TaskSummary): void {
    this.tasksSignal.update((items) => {
      const existingIndex = items.findIndex(
        (item) => item.taskId === summary.taskId,
      );
      if (existingIndex >= 0) {
        const copy = [...items];
        copy[existingIndex] = summary;
        return copy;
      }
      return [...items, summary].sort((a, b) =>
        a.createdAt.localeCompare(b.createdAt),
      );
    });
    if (!this.selectedTaskIdSignal()) {
      this.selectedTaskIdSignal.set(summary.taskId);
    }
  }

  private removeTask(taskId: string): void {
    this.tasksSignal.update((items) =>
      items.filter((item) => item.taskId !== taskId),
    );
    if (this.selectedTaskIdSignal() === taskId) {
      this.selectedTaskIdSignal.set(null);
    }
    this.terminalBuffers.delete(taskId);
    this.terminalStreams.delete(taskId);
    this.worktreeTerminalBuffers.delete(taskId);
    this.worktreeTerminalStreams.delete(taskId);
    this.worktreeTerminalOpenState.delete(taskId);
  }

  private pushTerminalOutput(taskId: string, chunk: string): void {
    const buffer = this.terminalBuffers.get(taskId) ?? "";
    this.terminalBuffers.set(taskId, buffer + chunk);
    const stream = this.ensureTerminalStream(taskId);
    stream.next(chunk);
  }

  private pushWorktreeTerminalOutput(taskId: string, chunk: string): void {
    const buffer = this.worktreeTerminalBuffers.get(taskId) ?? "";
    this.worktreeTerminalBuffers.set(taskId, buffer + chunk);
    const stream = this.ensureWorktreeTerminalStream(taskId);
    stream.next(chunk);
  }

  private async loadExistingTasks(baseRepoPath: string): Promise<void> {
    try {
      const summaries = await invoke<TaskSummary[]>("load_existing_worktrees", {
        baseRepoPath,
      });
      summaries.forEach((summary) => this.upsertTask(summary));
    } catch (error) {
      console.error("Failed to load existing worktrees", error);
    }
  }

  private async loadBranches(baseRepoPath: string): Promise<void> {
    try {
      const branches = await this.taskGit.listBranches(baseRepoPath);
      this.branchOptionsSignal.set(branches);
    } catch (error) {
      console.error("Failed to load branches", error);
      this.branchOptionsSignal.set([]);
    }
  }

  private ensureTerminalStream(taskId: string): Subject<string> {
    if (!this.terminalStreams.has(taskId)) {
      this.terminalStreams.set(taskId, new Subject<string>());
    }
    return this.terminalStreams.get(taskId)!;
  }

  private ensureWorktreeTerminalStream(taskId: string): Subject<string> {
    if (!this.worktreeTerminalStreams.has(taskId)) {
      this.worktreeTerminalStreams.set(taskId, new Subject<string>());
    }
    return this.worktreeTerminalStreams.get(taskId)!;
  }

  private teardown(): void {
    while (this.unlistenFns.length > 0) {
      const unlisten = this.unlistenFns.pop();
      if (unlisten) {
        void unlisten();
      }
    }
  }
}
