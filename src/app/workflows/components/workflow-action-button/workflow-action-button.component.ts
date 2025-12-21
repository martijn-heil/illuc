import { CommonModule } from "@angular/common";
import { Component, EventEmitter, Input, Output } from "@angular/core";

export type WorkflowActionButtonType = "start" | "stop" | "discard";

@Component({
  selector: "app-workflow-action-button",
  standalone: true,
  imports: [CommonModule],
  templateUrl: "./workflow-action-button.component.html",
  styleUrl: "./workflow-action-button.component.css",
})
export class WorkflowActionButtonComponent {
  @Input({ required: true }) type: WorkflowActionButtonType = "start";
  @Input() disabled = false;
  @Input() title?: string;
  @Input() ariaLabel?: string;
  @Input() stopPropagation = false;
  @Output() action = new EventEmitter<void>();

  handleClick(event: MouseEvent): void {
    if (this.disabled) {
      return;
    }
    if (this.stopPropagation) {
      event.stopPropagation();
    }
    this.action.emit();
  }

  get computedTitle(): string {
    if (this.title) {
      return this.title;
    }
    switch (this.type) {
      case "start":
        return "Start workflow";
      case "stop":
        return "Stop workflow";
      case "discard":
        return "Discard workflow";
      default:
        return "";
    }
  }

  get computedAriaLabel(): string {
    if (this.ariaLabel) {
      return this.ariaLabel;
    }
    return this.computedTitle;
  }
}
