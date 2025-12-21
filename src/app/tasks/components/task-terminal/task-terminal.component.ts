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
  @ViewChild("terminalHost", { static: true })
  terminalHost?: ElementRef<HTMLDivElement>;

  private terminal?: Terminal;
  private fitAddon?: FitAddon;
  private dataSubscription?: Subscription;
  private resizeObserver?: ResizeObserver;

  constructor(private readonly taskStore: TaskStore) {}

  ngAfterViewInit(): void {
    this.initializeTerminal();
    this.refreshTerminalSession();
    this.setupResizeObserver();
  }

  ngOnChanges(changes: SimpleChanges): void {
    if (changes["taskId"] && !changes["taskId"].firstChange) {
      this.refreshTerminalSession();
    }
  }

  ngOnDestroy(): void {
    this.dataSubscription?.unsubscribe();
    this.resizeObserver?.disconnect();
    this.terminal?.dispose();
  }

  async clear(): Promise<void> {
    if (this.taskId) {
      this.taskStore.clearTerminal(this.taskId);
    }
    this.terminal?.reset();
  }

  async copySelection(): Promise<void> {
    const selection = this.terminal?.getSelection();
    if (!selection) {
      return;
    }
    try {
      await navigator.clipboard.writeText(selection);
    } catch (error) {
      console.warn("Failed to copy terminal selection", error);
    }
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
        background: "#090b10",
        foreground: "#f8f9ff",
        cursor: "#5ce1e6",
      },
    });

    this.fitAddon = new FitAddon();
    this.terminal.loadAddon(this.fitAddon);
    this.terminal.open(this.terminalHost.nativeElement);
    this.terminal.focus();
    this.terminal.onData((data) => this.handleTerminalInput(data));
    this.terminal.onResize((size) => this.handleResize(size.cols, size.rows));
    this.fitTerminal();
  }

  private refreshTerminalSession(): void {
    if (!this.terminal) {
      return;
    }
    this.dataSubscription?.unsubscribe();
    this.terminal.reset();

    if (!this.taskId) {
      return;
    }

    const buffer = this.taskStore.getTerminalBuffer(this.taskId);
    if (buffer) {
      this.terminal.write(buffer);
    }

    this.dataSubscription = this.taskStore
      .terminalOutput$(this.taskId)
      .subscribe((chunk) => {
        this.terminal?.write(chunk);
      });
    this.fitTerminal();
  }

  private fitTerminal(): void {
    this.fitAddon?.fit();
    if (this.taskId && this.terminal) {
      void this.taskStore.resizeTaskTerminal(
        this.taskId,
        this.terminal.cols,
        this.terminal.rows,
      );
    }
  }

  private setupResizeObserver(): void {
    if (!this.terminalHost || typeof ResizeObserver === "undefined") {
      return;
    }
    this.resizeObserver = new ResizeObserver(() => this.fitTerminal());
    this.resizeObserver.observe(this.terminalHost.nativeElement);
  }

  private handleTerminalInput(data: string): void {
    if (!this.taskId) {
      return;
    }
    void this.taskStore.writeToTask(this.taskId, data);
  }

  private handleResize(cols: number, rows: number): void {
    if (!this.taskId) {
      return;
    }
    void this.taskStore.resizeTaskTerminal(this.taskId, cols, rows);
  }
}
