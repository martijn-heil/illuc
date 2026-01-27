import {
    AfterViewInit,
    Component,
    Input,
    OnChanges,
    OnDestroy,
    SimpleChanges,
    ViewChild,
    ElementRef,
} from "@angular/core";
import { CommonModule } from "@angular/common";
import { Terminal } from "xterm";
import { FitAddon } from "xterm-addon-fit";
import { Subscription } from "rxjs";
import { TaskStore } from "../../../task.store";
import { TERMINAL_SCROLLBACK } from "../../../terminal.constants";
import { TerminalFitManager } from "../../../terminal-fit.util";
import { TerminalKind } from "../../../task.models";

@Component({
    selector: "app-task-terminal",
    standalone: true,
    imports: [CommonModule],
    templateUrl: "./task-terminal.component.html",
    styleUrl: "./task-terminal.component.css",
})
export class TaskTerminalComponent
    implements AfterViewInit, OnChanges, OnDestroy
{
    @Input() taskId: string | null = null;
    @Input() mode: "agent" | "worktree" = "agent";
    @Input() title = "Terminal";
    @Input() showToolbar = true;
    @Input() suspendBackendResize = false;
    @Input() suspendFit = false;
    @ViewChild("terminalHost", { static: true })
    terminalHost?: ElementRef<HTMLDivElement>;

    private terminal?: Terminal;
    private fitAddon?: FitAddon;
    private fitManager?: TerminalFitManager;
    private dataSubscription?: Subscription;
    private resizeObserver?: ResizeObserver;
    private wheelHandler?: (event: WheelEvent) => void;
    private resizeTimer?: number;
    private pendingResize?: { cols: number; rows: number };
    private altScreenActive = false;
    private altScreenCarry = "";
    private readonly altScreenSequences = [
        "\u001b[?1049h",
        "\u001b[?1049l",
        "\u001b[?1047h",
        "\u001b[?1047l",
        "\u001b[?47h",
        "\u001b[?47l",
    ];
    private readonly altScreenMaxLen = 8;
    private readonly isWindows = navigator.userAgent
        .toLowerCase()
        .includes("windows");
    private readonly assumeAltScreenOnWindows = true;

    constructor(private readonly taskStore: TaskStore) {}

    ngAfterViewInit(): void {
        this.initializeTerminal();
        this.refreshTerminalSession();
        this.setupResizeObserver();
        this.fitManager?.scheduleFit();
    }

    ngOnChanges(changes: SimpleChanges): void {
        if (
            (changes["taskId"] && !changes["taskId"].firstChange) ||
            (changes["mode"] && !changes["mode"].firstChange)
        ) {
            this.refreshTerminalSession();
        }
    }

    ngOnDestroy(): void {
        this.dataSubscription?.unsubscribe();
        this.resizeObserver?.disconnect();
        if (this.wheelHandler && this.terminal?.element) {
            this.terminal.element.removeEventListener(
                "wheel",
                this.wheelHandler,
            );
        }
        if (this.resizeTimer) {
            window.clearTimeout(this.resizeTimer);
        }
        this.terminal?.dispose();
    }

    private initializeTerminal(): void {
        if (!this.terminalHost) {
            return;
        }

        this.terminal = new Terminal({
            convertEol: false,
            fontFamily:
                "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace",
            fontSize: 13,
            cursorBlink: true,
            scrollback: TERMINAL_SCROLLBACK,
            theme: {
                background: "#f7f3ec",
                foreground: "#4f4942",
                cursor: "#b2714a",
                white: "#4f4942",
                brightWhite: "#4f4942",
            },
        });

        this.fitAddon = new FitAddon();
        this.terminal.loadAddon(this.fitAddon);
        this.terminal.open(this.terminalHost.nativeElement);
        this.terminal.focus();
        this.fitManager = new TerminalFitManager(
            () => this.fitAddon?.proposeDimensions(),
            () => this.fitTerminal(),
            () => this.suspendFit,
        );
        this.fitManager.scheduleFit();
        this.wheelHandler = (event: WheelEvent) =>
            this.handleTerminalWheel(event);
        if (this.terminal.element) {
            this.terminal.element.addEventListener("wheel", this.wheelHandler, {
                passive: false,
            });
        }
        this.terminal.onData((data) => this.handleTerminalInput(data));
        this.terminal.onResize((size) =>
            this.handleResize(size.cols, size.rows),
        );
        this.fitManager.scheduleFit();
    }

    private refreshTerminalSession(): void {
        if (!this.terminal) {
            return;
        }
        this.dataSubscription?.unsubscribe();
        this.altScreenActive = this.isWindows && this.assumeAltScreenOnWindows;
        this.altScreenCarry = "";
        this.terminal.reset();

        if (!this.taskId) {
            return;
        }

        if (this.mode === "worktree") {
            void this.taskStore
                .startTerminal(this.taskId, "worktree")
                .then(() => this.forceBackendResizeNow())
                .catch(() => undefined);
        }

        const buffer = this.taskStore.getTerminalBuffer(
            this.taskId,
            this.terminalKind(),
        );
        if (buffer) {
            this.terminal.write(buffer);
        }

        const output$ = this.taskStore.terminalOutput$(
            this.taskId,
            this.terminalKind(),
        );
        this.dataSubscription = output$.subscribe((chunk) => {
            this.detectAltScreen(chunk);
            this.terminal?.write(chunk);
        });
        this.fitManager?.scheduleFit();
    }

    private fitTerminal(): void {
        this.fitAddon?.fit();
        if (!this.terminal) {
            return;
        }
        const cols = this.terminal.cols;
        const rows = this.terminal.rows;
        this.recordTerminalSize(cols, rows);
        this.scheduleBackendResize(cols, rows);
    }

    private setupResizeObserver(): void {
        if (!this.terminalHost || typeof ResizeObserver === "undefined") {
            return;
        }
        this.resizeObserver = new ResizeObserver(() =>
            this.fitManager?.scheduleFitOnResize(),
        );
        this.resizeObserver.observe(this.terminalHost.nativeElement);
    }

    private handleTerminalInput(data: string): void {
        if (!this.taskId) {
            return;
        }
        if (this.mode === "worktree") {
            void this.taskStore.writeToTerminal(this.taskId, data, "worktree");
            return;
        }
        void this.taskStore.writeToTask(this.taskId, data);
    }

    private handleResize(cols: number, rows: number): void {
        if (!this.taskId) {
            return;
        }
        this.recordTerminalSize(cols, rows);
        this.scheduleBackendResize(cols, rows);
    }

    private recordTerminalSize(cols: number, rows: number): void {
        this.taskStore.recordTerminalSize(
            this.taskId ?? "",
            cols,
            rows,
            this.terminalKind(),
        );
    }

    private sendBackendResize(cols: number, rows: number): void {
        if (!this.taskId) {
            return;
        }
        if (this.mode === "worktree") {
            void this.taskStore.resizeTerminal(
                this.taskId,
                cols,
                rows,
                "worktree",
            );
            return;
        }
        void this.taskStore.resizeTaskTerminal(this.taskId, cols, rows);
    }

    private scheduleBackendResize(cols: number, rows: number): void {
        if (!this.taskId) {
            return;
        }
        this.pendingResize = { cols, rows };
        if (this.suspendBackendResize) {
            return;
        }
        if (this.resizeTimer) {
            window.clearTimeout(this.resizeTimer);
        }
        this.resizeTimer = window.setTimeout(() => {
            this.flushBackendResize();
        }, 150);
    }

    flushBackendResize(): void {
        const payload = this.pendingResize;
        this.pendingResize = undefined;
        if (this.resizeTimer) {
            window.clearTimeout(this.resizeTimer);
            this.resizeTimer = undefined;
        }
        if (!payload || !this.taskId) {
            return;
        }
        this.sendBackendResize(payload.cols, payload.rows);
    }

    runFitNow(): void {
        this.fitTerminal();
    }

    forceBackendResizeNow(ignoreSuspend = false): void {
        if (!ignoreSuspend && (this.suspendBackendResize || this.suspendFit)) {
            return;
        }
        if (!this.terminal) {
            return;
        }
        this.fitAddon?.fit();
        const cols = this.terminal.cols;
        const rows = this.terminal.rows;
        this.recordTerminalSize(cols, rows);
        this.sendBackendResize(cols, rows);
    }

    private handleTerminalWheel(event: WheelEvent): boolean {
        if (
            !this.isWindows ||
            (!this.assumeAltScreenOnWindows && !this.altScreenActive) ||
            !this.taskId
        ) {
            return true;
        }
        event.preventDefault();
        const sequence = event.deltaY < 0 ? "\u001b[5~" : "\u001b[6~";
        const repeats = Math.min(
            3,
            Math.max(1, Math.round(Math.abs(event.deltaY) / 100)),
        );
        void this.taskStore.writeToTask(this.taskId, sequence.repeat(repeats));
        return false;
    }

    private terminalKind(): TerminalKind {
        return this.mode === "worktree" ? "worktree" : "agent";
    }

    private detectAltScreen(chunk: string): void {
        if (!this.isWindows) {
            return;
        }
        if (this.assumeAltScreenOnWindows) {
            this.altScreenActive = true;
            return;
        }
        const combined = this.altScreenCarry + chunk;
        let lastMatchIndex = -1;
        let lastMatchValue = "";
        for (const sequence of this.altScreenSequences) {
            const index = combined.lastIndexOf(sequence);
            if (index > lastMatchIndex) {
                lastMatchIndex = index;
                lastMatchValue = sequence;
            }
        }
        if (lastMatchIndex >= 0) {
            this.altScreenActive = lastMatchValue.endsWith("h");
        }
        this.altScreenCarry = this.pendingAltScreenSuffix(combined);
    }

    private pendingAltScreenSuffix(value: string): string {
        const maxLen = Math.min(this.altScreenMaxLen, value.length);
        for (let len = maxLen; len > 0; len -= 1) {
            const suffix = value.slice(value.length - len);
            if (
                this.altScreenSequences.some((sequence) =>
                    sequence.startsWith(suffix),
                )
            ) {
                return suffix;
            }
        }
        return "";
    }
}
