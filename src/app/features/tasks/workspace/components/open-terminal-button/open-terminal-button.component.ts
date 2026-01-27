import { CommonModule } from "@angular/common";
import { Component, Input } from "@angular/core";
import { LauncherService } from "../../../../launcher/launcher.service";
import { LoadingButtonComponent } from "../../../../../shared/components/loading-button/loading-button.component";

@Component({
    selector: "app-open-terminal-button",
    standalone: true,
    imports: [CommonModule, LoadingButtonComponent],
    templateUrl: "./open-terminal-button.component.html",
    styleUrl: "./open-terminal-button.component.css",
})
export class OpenTerminalButtonComponent {
    @Input() path: string | null = null;
    @Input() title = "Open terminal";
    @Input() ariaLabel = "Open terminal";
    isLoading = false;

    constructor(private readonly launcher: LauncherService) {}

    async handleClick(): Promise<void> {
        if (!this.path || this.isLoading) {
            return;
        }
        this.isLoading = true;
        try {
            await this.launcher.openTerminal(this.path);
        } catch (error) {
            console.error("Failed to open terminal", error);
        } finally {
            this.isLoading = false;
        }
    }
}
