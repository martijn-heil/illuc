import { CommonModule } from "@angular/common";
import { Component, EventEmitter, Input, Output } from "@angular/core";

@Component({
    selector: "app-loading-button",
    standalone: true,
    imports: [CommonModule],
    templateUrl: "./loading-button.component.html",
    styleUrl: "./loading-button.component.css",
})
export class LoadingButtonComponent {
    @Input() loading = false;
    @Input() disabled = false;
    @Input() buttonType: "button" | "submit" | "reset" = "button";
    @Input() ariaLabel?: string;
    @Input() ariaExpanded?: boolean | null;
    @Input() ariaHaspopup?: string | null;
    @Input() title?: string;
    @Input() dataAction?: string | null;
    @Input() buttonClass = "";
    @Input() stopPropagation = false;
    @Output() action = new EventEmitter<MouseEvent>();

    handleClick(event: MouseEvent): void {
        if (this.disabled || this.loading) {
            return;
        }
        if (this.stopPropagation) {
            event.stopPropagation();
        }
        this.action.emit(event);
    }
}
