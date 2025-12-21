import { CommonModule } from "@angular/common";
import { Component, EventEmitter, Input, Output } from "@angular/core";
import { WorkflowSummary } from "../../workflow.models";
import { parseTitleParts, TitleParts } from "../../title.utils";
import { WorkflowTerminalComponent } from "../workflow-terminal/workflow-terminal.component";
import { WorkflowDiffComponent } from "../workflow-diff/workflow-diff.component";

@Component({
  selector: "app-workflow-view",
  standalone: true,
  imports: [CommonModule, WorkflowTerminalComponent, WorkflowDiffComponent],
  templateUrl: "./workflow-view.component.html",
  styleUrl: "./workflow-view.component.css",
})
export class WorkflowViewComponent {
  @Input() workflow: WorkflowSummary | null = null;
  @Output() startWorkflow = new EventEmitter<string>();
  @Output() stopWorkflow = new EventEmitter<string>();
  @Output() discardWorkflow = new EventEmitter<string>();
  @Output() openInVsCode = new EventEmitter<string>();
  @Output() openTerminal = new EventEmitter<string>();

  statusLabel(): string {
    return this.workflow?.status.replace(/_/g, " ") ?? "";
  }

  canStart(): boolean {
    return !!this.workflow &&
      ["READY", "STOPPED", "COMPLETED", "FAILED"].includes(this.workflow.status);
  }

  isRunning(): boolean {
    return this.workflow?.status === "RUNNING";
  }

  titleParts(): TitleParts | null {
    if (!this.workflow) {
      return null;
    }
    return parseTitleParts(this.workflow.title);
  }

  onStart(): void {
    if (this.workflow) {
      this.startWorkflow.emit(this.workflow.workflowId);
    }
  }

  onStop(): void {
    if (this.workflow) {
      this.stopWorkflow.emit(this.workflow.workflowId);
    }
  }

  onDiscard(): void {
    if (this.workflow) {
      this.discardWorkflow.emit(this.workflow.workflowId);
    }
  }

  onOpenVsCode(): void {
    if (this.workflow) {
      this.openInVsCode.emit(this.workflow.workflowId);
    }
  }

  onOpenTerminal(): void {
    if (this.workflow) {
      this.openTerminal.emit(this.workflow.workflowId);
    }
  }
}
