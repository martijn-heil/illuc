import { CommonModule } from "@angular/common";
import { Component } from "@angular/core";
import { FormsModule } from "@angular/forms";
import { open } from "@tauri-apps/plugin-dialog";
import { WorkflowSidebarComponent } from "./workflows/components/workflow-sidebar/workflow-sidebar.component";
import { WorkflowViewComponent } from "./workflows/components/workflow-view/workflow-view.component";
import { WorkflowStore } from "./workflows/workflow.store";

@Component({
  selector: "app-root",
  standalone: true,
  imports: [
    CommonModule,
    FormsModule,
    WorkflowSidebarComponent,
    WorkflowViewComponent,
  ],
  templateUrl: "./app.component.html",
  styleUrl: "./app.component.css",
})
export class AppComponent {
  statusMessage = "";
  showCreateModal = false;
  branchNameInput = "";
  branchNameError = "";

  constructor(public readonly workflowStore: WorkflowStore) {}

  workflows() {
    return this.workflowStore.workflows();
  }

  selectedWorkflow() {
    return this.workflowStore.selectedWorkflow();
  }

  baseRepo() {
    return this.workflowStore.baseRepo();
  }

  async browseForRepo(): Promise<void> {
    const selection = await open({
        directory: true,
        multiple: false,
        title: "Select base repository",
    });
    if (typeof selection === "string") {
      await this.loadBaseRepo(selection);
    }
  }

  private async loadBaseRepo(path: string): Promise<void> {
    try {
      await this.workflowStore.selectBaseRepo(path);
      this.statusMessage = `Loaded repository: ${path}`;
    } catch (error: unknown) {
      this.statusMessage = this.describeError(
        error,
        "Unable to open the selected repository.",
      );
    }
  }

  openCreateWorkflowModal(): void {
    if (!this.baseRepo()) {
      this.statusMessage = "Select a base repository before creating workflows.";
      return;
    }
    this.branchNameInput = "";
    this.branchNameError = "";
    this.showCreateModal = true;
  }

  closeCreateWorkflowModal(): void {
    this.showCreateModal = false;
    this.branchNameInput = "";
    this.branchNameError = "";
  }

  async submitNewWorkflow(): Promise<void> {
    const branch = this.branchNameInput.trim();
    if (!branch) {
      this.branchNameError = "Branch name is required.";
      return;
    }
    if (!this.baseRepo()) {
      this.branchNameError = "Select a base repository first.";
      return;
    }
    this.branchNameError = "";
    const title = this.deriveTitleFromBranch(branch);
    try {
      await this.workflowStore.createWorkflow(branch, title);
      this.statusMessage = `Workflow created on ${branch}.`;
      this.closeCreateWorkflowModal();
    } catch (error: unknown) {
      this.branchNameError = this.describeError(
        error,
        "Unable to create workflow.",
      );
    }
  }

  async startWorkflow(workflowId: string): Promise<void> {
    try {
      await this.workflowStore.startWorkflow(workflowId);
      this.statusMessage = "Workflow started.";
    } catch (error: unknown) {
      this.statusMessage = this.describeError(
        error,
        "Unable to start workflow.",
      );
    }
  }

  async stopWorkflow(workflowId: string): Promise<void> {
    try {
      await this.workflowStore.stopWorkflow(workflowId);
      this.statusMessage = "Workflow stopped.";
    } catch (error: unknown) {
      this.statusMessage = this.describeError(error, "Unable to stop workflow.");
    }
  }

  async discardWorkflow(workflowId: string): Promise<void> {
    try {
      await this.workflowStore.discardWorkflow(workflowId);
      this.statusMessage = "Workflow discarded and cleaned up.";
    } catch (error: unknown) {
      this.statusMessage = this.describeError(
        error,
        "Unable to discard workflow.",
      );
    }
  }

  selectWorkflow(workflowId: string): void {
    this.workflowStore.selectWorkflow(workflowId);
  }

  async openWorkflowInVsCode(workflowId: string): Promise<void> {
    try {
      await this.workflowStore.openWorkflowInVsCode(workflowId);
      this.statusMessage = "Opened workspace in VS Code.";
    } catch (error: unknown) {
      this.statusMessage = this.describeError(
        error,
        "Unable to open workspace in VS Code.",
      );
    }
  }

  async openWorkflowTerminal(workflowId: string): Promise<void> {
    try {
      await this.workflowStore.openWorkflowTerminal(workflowId);
      this.statusMessage = "Opened workspace terminal.";
    } catch (error: unknown) {
      this.statusMessage = this.describeError(
        error,
        "Unable to open workspace terminal.",
      );
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

  private deriveTitleFromBranch(branchName: string): string {
    const slug = branchName.split("/").pop() ?? branchName;
    const taskMatch = slug.match(/(\d{3,})/);
    const taskId = taskMatch ? taskMatch[1] : null;
    let remainder = slug;
    if (taskMatch) {
      remainder = remainder.replace(taskMatch[1], "");
    }
    remainder = remainder.replace(/[-_]+/g, " ").replace(/\s+/g, " ").trim();
    if (!remainder) {
      remainder = branchName.replace(/[-_/]+/g, " ").trim();
    }
    const human = remainder
      ? remainder
          .split(" ")
          .filter(Boolean)
          .map((word) => word.charAt(0).toUpperCase() + word.slice(1).toLowerCase())
          .join(" ")
      : branchName;
    return taskId ? `[${taskId}] ${human}` : human;
  }
}
