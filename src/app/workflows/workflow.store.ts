import { Injectable, NgZone, computed, signal } from "@angular/core";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { Observable, Subject } from "rxjs";
import {
  BaseRepoInfo,
  DiffPayload,
  TerminalExitEvent,
  TerminalOutputEvent,
  WorkflowSummary,
} from "./workflow.models";

@Injectable({
  providedIn: "root",
})
export class WorkflowStore {
  private readonly workflowsSignal = signal<WorkflowSummary[]>([]);
  private readonly baseRepoSignal = signal<BaseRepoInfo | null>(null);
  private readonly selectedWorkflowIdSignal = signal<string | null>(null);
  private readonly terminalBuffers = new Map<string, string>();
  private readonly terminalStreams = new Map<string, Subject<string>>();
  private readonly unlistenFns: UnlistenFn[] = [];

  readonly workflows = this.workflowsSignal.asReadonly();
  readonly baseRepo = this.baseRepoSignal.asReadonly();
  readonly selectedWorkflowId = this.selectedWorkflowIdSignal.asReadonly();
  readonly selectedWorkflow = computed(() => {
    const id = this.selectedWorkflowIdSignal();
    if (!id) {
      return null;
    }
    return this.workflowsSignal().find((wf) => wf.workflowId === id) ?? null;
  });

  constructor(private readonly zone: NgZone) {
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
    this.workflowsSignal.set([]);
    this.selectedWorkflowIdSignal.set(null);
    this.terminalBuffers.clear();
    this.terminalStreams.clear();
    await this.loadExistingWorkflows(normalized.path);
    return normalized;
  }

  async createWorkflow(branchName: string, displayTitle: string): Promise<WorkflowSummary> {
    const repo = this.baseRepoSignal();
    if (!repo) {
      throw new Error("Select a base repository before creating workflows.");
    }
    const summary = await invoke<WorkflowSummary>("create_workflow", {
      req: {
        baseRepoPath: repo.path,
        baseRef: repo.head,
        taskTitle: displayTitle.trim() || undefined,
        branchName: branchName.trim(),
      },
    });
    this.upsertWorkflow(summary);
    this.selectedWorkflowIdSignal.set(summary.workflowId);
    return summary;
  }

  async openWorkflowInVsCode(workflowId: string): Promise<void> {
    await invoke("open_worktree_in_vscode", {
      req: { workflowId },
    });
  }

  async openWorkflowTerminal(workflowId: string): Promise<void> {
    await invoke("open_worktree_terminal", {
      req: { workflowId },
    });
  }

  async startWorkflow(workflowId: string, codexArgs?: string[]): Promise<WorkflowSummary> {
    const summary = await invoke<WorkflowSummary>("start_workflow", {
      req: {
        workflowId,
        codexArgs,
      },
    });
    this.upsertWorkflow(summary);
    return summary;
  }

  async stopWorkflow(workflowId: string): Promise<WorkflowSummary> {
    const summary = await invoke<WorkflowSummary>("stop_workflow", {
      req: { workflowId },
    });
    this.upsertWorkflow(summary);
    return summary;
  }

  async discardWorkflow(workflowId: string): Promise<void> {
    await invoke("discard_workflow", { req: { workflowId } });
    this.removeWorkflow(workflowId);
  }

  async writeToWorkflow(workflowId: string, data: string): Promise<void> {
    await invoke("terminal_write", { req: { workflowId, data } });
  }

  async resizeWorkflowTerminal(
    workflowId: string,
    cols: number,
    rows: number,
  ): Promise<void> {
    await invoke("terminal_resize", {
      req: {
        workflowId,
        cols,
        rows,
      },
    });
  }

  async getDiff(workflowId: string, ignoreWhitespace = false): Promise<DiffPayload> {
    return invoke<DiffPayload>("get_diff", {
      req: {
        workflowId,
        ignoreWhitespace,
      },
    });
  }

  selectWorkflow(workflowId: string | null): void {
    this.selectedWorkflowIdSignal.set(workflowId);
  }

  getTerminalBuffer(workflowId: string): string {
    return this.terminalBuffers.get(workflowId) ?? "";
  }

  terminalOutput$(workflowId: string): Observable<string> {
    const stream = this.ensureTerminalStream(workflowId);
    return stream.asObservable();
  }

  clearTerminal(workflowId: string): void {
    this.terminalBuffers.set(workflowId, "");
    const stream = this.ensureTerminalStream(workflowId);
    stream.next("\u001bc");
  }

  private registerEventListeners(): void {
    void listen<WorkflowSummary>("workflow_status_changed", (event) => {
      this.zone.run(() => {
        this.upsertWorkflow(event.payload);
      });
    }).then((unlisten) => this.unlistenFns.push(unlisten));

    void listen<TerminalOutputEvent>("workflow_terminal_output", (event) => {
      this.zone.run(() => {
        this.pushTerminalOutput(event.payload.workflowId, event.payload.data);
      });
    }).then((unlisten) => this.unlistenFns.push(unlisten));

    void listen<TerminalExitEvent>("workflow_terminal_exit", (event) => {
      this.zone.run(() => {
        console.info(
          `Workflow ${event.payload.workflowId} exited with code ${event.payload.exitCode}`,
        );
      });
    }).then((unlisten) => this.unlistenFns.push(unlisten));
  }

  private upsertWorkflow(summary: WorkflowSummary): void {
    this.workflowsSignal.update((items) => {
      const existingIndex = items.findIndex(
        (item) => item.workflowId === summary.workflowId,
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
    if (!this.selectedWorkflowIdSignal()) {
      this.selectedWorkflowIdSignal.set(summary.workflowId);
    }
  }

  private removeWorkflow(workflowId: string): void {
    this.workflowsSignal.update((items) =>
      items.filter((item) => item.workflowId !== workflowId),
    );
    if (this.selectedWorkflowIdSignal() === workflowId) {
      this.selectedWorkflowIdSignal.set(null);
    }
    this.terminalBuffers.delete(workflowId);
    this.terminalStreams.delete(workflowId);
  }

  private pushTerminalOutput(workflowId: string, chunk: string): void {
    const buffer = this.terminalBuffers.get(workflowId) ?? "";
    this.terminalBuffers.set(workflowId, buffer + chunk);
    const stream = this.ensureTerminalStream(workflowId);
    stream.next(chunk);
  }

  private async loadExistingWorkflows(baseRepoPath: string): Promise<void> {
    try {
      const summaries = await invoke<WorkflowSummary[]>("load_existing_worktrees", {
        baseRepoPath,
      });
      summaries.forEach((summary) => this.upsertWorkflow(summary));
    } catch (error) {
      console.error("Failed to load existing worktrees", error);
    }
  }

  private ensureTerminalStream(workflowId: string): Subject<string> {
    if (!this.terminalStreams.has(workflowId)) {
      this.terminalStreams.set(workflowId, new Subject<string>());
    }
    return this.terminalStreams.get(workflowId)!;
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
