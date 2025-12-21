import { CommonModule } from "@angular/common";
import { Component, EventEmitter, Input, Output } from "@angular/core";
import { WorkflowStatus, WorkflowSummary, BaseRepoInfo } from "../../workflow.models";
import { parseTitleParts, TitleParts } from "../../title.utils";

@Component({
  selector: "app-workflow-sidebar",
  standalone: true,
  imports: [CommonModule],
  templateUrl: "./workflow-sidebar.component.html",
  styleUrl: "./workflow-sidebar.component.css",
})
export class WorkflowSidebarComponent {
  @Input({ required: true }) workflows: WorkflowSummary[] | null = [];
  @Input() selectedWorkflowId: string | null = null;
  @Input() baseRepo: BaseRepoInfo | null = null;
  @Output() selectWorkflow = new EventEmitter<string>();
  @Output() startWorkflow = new EventEmitter<string>();
  @Output() stopWorkflow = new EventEmitter<string>();
  @Output() discardWorkflow = new EventEmitter<string>();
  @Output() browseRepo = new EventEmitter<void>();
  @Output() createWorkflow = new EventEmitter<void>();

  trackById(_: number, workflow: WorkflowSummary): string {
    return workflow.workflowId;
  }

  onSelect(workflowId: string): void {
    this.selectWorkflow.emit(workflowId);
  }

  onStart(workflowId: string, event: MouseEvent): void {
    event.stopPropagation();
    this.startWorkflow.emit(workflowId);
  }

  onStop(workflowId: string, event: MouseEvent): void {
    event.stopPropagation();
    this.stopWorkflow.emit(workflowId);
  }

  onDiscard(workflowId: string, event: MouseEvent): void {
    event.stopPropagation();
    this.discardWorkflow.emit(workflowId);
  }

  statusLabel(status: WorkflowStatus): string {
    return status.replace(/_/g, " ");
  }

  canStart(status: WorkflowStatus): boolean {
    return (
      status === "READY" ||
      status === "STOPPED" ||
      status === "FAILED" ||
      status === "COMPLETED"
    );
  }

  isRunning(status: WorkflowStatus): boolean {
    return status === "RUNNING";
  }

  titleParts(title: string): TitleParts {
    return parseTitleParts(title);
  }
}
