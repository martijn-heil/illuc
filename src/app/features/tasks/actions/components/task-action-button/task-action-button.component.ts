import { CommonModule } from "@angular/common";
import { Component, EventEmitter, Input, Output } from "@angular/core";
import { LoadingButtonComponent } from "../../../../../shared/components/loading-button/loading-button.component";

export type TaskActionButtonType = "stop" | "discard" | "commit" | "push";

@Component({
    selector: "app-task-action-button",
    standalone: true,
    imports: [CommonModule, LoadingButtonComponent],
    templateUrl: "./task-action-button.component.html",
    styleUrl: "./task-action-button.component.css",
})
export class TaskActionButtonComponent {
    @Input({ required: true }) type: TaskActionButtonType = "stop";
    @Input() disabled = false;
    @Input() loading = false;
    @Input() title?: string;
    @Input() ariaLabel?: string;
    @Input() stopPropagation = false;
    @Output() action = new EventEmitter<void>();

    handleClick(event: MouseEvent): void {
        if (this.disabled || this.loading) {
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
            case "stop":
                return "Stop task";
            case "discard":
                return "Discard task";
            case "commit":
                return "Commit changes";
            case "push":
                return "Push changes";
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

    get buttonClass(): string {
        return `action-btn${this.type === "discard" ? " warn" : ""}`;
    }
}
