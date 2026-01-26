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
import { TaskStore } from "../../task.store";

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
  private dataSubscription?: Subscription;
  private resizeObserver?: ResizeObserver;
  private wheelHandler?: (event: WheelEvent) => void;
  private resizeTimer?: number;
  private pendingResize?: { cols: number; rows: number };
  private fitScheduled = false;
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
  private readonly isWindows = navigator.userAgent.toLowerCase().includes("windows");
  private readonly assumeAltScreenOnWindows = true;

  constructor(private readonly taskStore: TaskStore) {}

  ngAfterViewInit(): void {
    this.initializeTerminal();
    this.refreshTerminalSession();
    this.setupResizeObserver();
    this.scheduleFit();
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
      this.terminal.element.removeEventListener("wheel", this.wheelHandler);
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
      fontFamily: "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace",
      fontSize: 13,
      cursorBlink: true,
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
    this.scheduleFit();
    this.wheelHandler = (event: WheelEvent) => this.handleTerminalWheel(event);
    if (this.terminal.element) {
      this.terminal.element.addEventListener("wheel", this.wheelHandler, {
        passive: false,
      });
    }
    this.terminal.onData((data) => this.handleTerminalInput(data));
    this.terminal.onResize((size) => this.handleResize(size.cols, size.rows));
    this.scheduleFit();
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
        .startWorktreeTerminal(this.taskId)
        .then(() => this.forceBackendResizeNow())
        .catch(() => undefined);
    }

    const buffer =
      this.mode === "worktree"
        ? this.taskStore.getWorktreeTerminalBuffer(this.taskId)
        : this.taskStore.getTerminalBuffer(this.taskId);
    if (buffer) {
      this.terminal.write(buffer);
    }

    const output$ =
      this.mode === "worktree"
        ? this.taskStore.worktreeTerminalOutput$(this.taskId)
        : this.taskStore.terminalOutput$(this.taskId);
    this.dataSubscription = output$.subscribe((chunk) => {
      this.detectAltScreen(chunk);
      this.terminal?.write(chunk);
    });
    this.scheduleFit();
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

  private scheduleFit(): void {
    if (this.suspendFit) {
      return;
    }
    this.scheduleFitAttempt(0);
    if ("fonts" in document) {
      void (document as Document & { fonts: FontFaceSet }).fonts.ready.then(() => {
        if (!this.suspendFit) {
          this.scheduleFitAttempt(0);
        }
      });
    }
  }

  private scheduleFitAttempt(attempt: number): void {
    const maxAttempts = 30;
    requestAnimationFrame(() => {
      const dimensions = this.fitAddon?.proposeDimensions();
      if (dimensions?.cols && dimensions.rows) {
        this.fitTerminal();
        return;
      }
      if (attempt < maxAttempts) {
        this.scheduleFitAttempt(attempt + 1);
      }
    });
  }

  private setupResizeObserver(): void {
    if (!this.terminalHost || typeof ResizeObserver === "undefined") {
      return;
    }
    this.resizeObserver = new ResizeObserver(() => this.scheduleFitOnResize());
    this.resizeObserver.observe(this.terminalHost.nativeElement);
  }

  private handleTerminalInput(data: string): void {
    if (!this.taskId) {
      return;
    }
    if (this.mode === "worktree") {
      void this.taskStore.writeToWorktreeTerminal(this.taskId, data);
    } else {
      void this.taskStore.writeToTask(this.taskId, data);
    }
  }

  private handleResize(cols: number, rows: number): void {
    if (!this.taskId) {
      return;
    }
    this.recordTerminalSize(cols, rows);
    this.scheduleBackendResize(cols, rows);
  }

  private recordTerminalSize(cols: number, rows: number): void {
    if (this.mode === "worktree") {
      this.taskStore.recordWorktreeTerminalSize(this.taskId ?? "", cols, rows);
    } else {
      this.taskStore.recordTerminalSize(this.taskId ?? "", cols, rows);
    }
  }

  private sendBackendResize(cols: number, rows: number): void {
    if (!this.taskId) {
      return;
    }
    if (this.mode === "worktree") {
      void this.taskStore.resizeWorktreeTerminal(this.taskId, cols, rows);
    } else {
      void this.taskStore.resizeTaskTerminal(this.taskId, cols, rows);
    }
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

  private scheduleFitOnResize(): void {
    if (this.fitScheduled) {
      return;
    }
    this.fitScheduled = true;
    requestAnimationFrame(() => {
      this.fitScheduled = false;
      if (!this.suspendFit) {
        this.fitTerminal();
      }
    });
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
    const repeats = Math.min(3, Math.max(1, Math.round(Math.abs(event.deltaY) / 100)));
    void this.taskStore.writeToTask(this.taskId, sequence.repeat(repeats));
    return false;
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
      if (this.altScreenSequences.some((sequence) => sequence.startsWith(suffix))) {
        return suffix;
      }
    }
    return "";
  }
}
