import { CommonModule } from "@angular/common";
import {
  Component,
  ElementRef,
  Input,
  OnChanges,
  OnDestroy,
  QueryList,
  SimpleChanges,
  ViewChildren,
} from "@angular/core";
import hljs from "highlight.js/lib/core";
import javascript from "highlight.js/lib/languages/javascript";
import json from "highlight.js/lib/languages/json";
import typescript from "highlight.js/lib/languages/typescript";
import xml from "highlight.js/lib/languages/xml";
import css from "highlight.js/lib/languages/css";
import scss from "highlight.js/lib/languages/scss";
import bash from "highlight.js/lib/languages/bash";
import python from "highlight.js/lib/languages/python";
import java from "highlight.js/lib/languages/java";
import go from "highlight.js/lib/languages/go";
import rust from "highlight.js/lib/languages/rust";
import yaml from "highlight.js/lib/languages/yaml";
import csharp from "highlight.js/lib/languages/csharp";
import c from "highlight.js/lib/languages/c";
import cpp from "highlight.js/lib/languages/cpp";
import markdown from "highlight.js/lib/languages/markdown";
import { DomSanitizer, SafeHtml } from "@angular/platform-browser";
import { EMPTY, Subscription, from, timer } from "rxjs";
import { catchError, switchMap } from "rxjs/operators";
import { DiffPayload } from "../../workflow.models";
import { WorkflowStore } from "../../workflow.store";

hljs.registerLanguage("javascript", javascript);
hljs.registerLanguage("typescript", typescript);
hljs.registerLanguage("json", json);
hljs.registerLanguage("xml", xml);
hljs.registerLanguage("html", xml);
hljs.registerLanguage("css", css);
hljs.registerLanguage("scss", scss);
hljs.registerLanguage("bash", bash);
hljs.registerLanguage("shell", bash);
hljs.registerLanguage("python", python);
hljs.registerLanguage("java", java);
hljs.registerLanguage("go", go);
hljs.registerLanguage("rust", rust);
hljs.registerLanguage("yaml", yaml);
hljs.registerLanguage("csharp", csharp);
hljs.registerLanguage("c", c);
hljs.registerLanguage("cpp", cpp);
hljs.registerLanguage("markdown", markdown);

type DiffLineType = "add" | "del" | "context" | "meta" | "hunk";

interface RenderedDiffLine {
  type: DiffLineType;
  html: SafeHtml;
}

interface RenderedDiffFile {
  path: string;
  status: string;
  lines: RenderedDiffLine[];
}

@Component({
  selector: "app-workflow-diff",
  standalone: true,
  imports: [CommonModule],
  templateUrl: "./workflow-diff.component.html",
  styleUrl: "./workflow-diff.component.css",
})
export class WorkflowDiffComponent implements OnChanges, OnDestroy {
  @Input() workflowId: string | null = null;
  @ViewChildren("diffSection") diffSections?: QueryList<ElementRef<HTMLElement>>;

  diffPayload: DiffPayload | null = null;
  renderedFiles: RenderedDiffFile[] = [];
  lastUpdated: Date | null = null;
  error: string | null = null;
  ignoreWhitespace = false;
  private polling?: Subscription;

  constructor(
    private readonly workflowStore: WorkflowStore,
    private readonly sanitizer: DomSanitizer,
  ) {}

  ngOnChanges(changes: SimpleChanges): void {
    if (changes["workflowId"]) {
      this.restartPolling();
    }
  }

  ngOnDestroy(): void {
    this.stopPolling();
  }

  toggleWhitespace(): void {
    this.ignoreWhitespace = !this.ignoreWhitespace;
    this.restartPolling();
  }

  refreshNow(): void {
    this.fetchDiffOnce();
  }

  private restartPolling(): void {
    this.stopPolling();
    this.diffPayload = null;
    this.renderedFiles = [];
    this.error = null;
    if (!this.workflowId) {
      return;
    }
    const workflowId = this.workflowId;
    this.polling = timer(0, 2000)
      .pipe(
        switchMap(() =>
          from(
            this.workflowStore.getDiff(workflowId, this.ignoreWhitespace),
          ).pipe(
            catchError((err) => {
              this.error =
                err?.message ??
                "Unable to load diff. The git repository may be inaccessible.";
              return EMPTY;
            }),
          ),
        ),
      )
      .subscribe((payload) => {
        this.diffPayload = payload;
        this.renderedFiles = this.buildRenderedDiff(payload);
        this.lastUpdated = new Date();
        this.error = null;
      });
  }

  private stopPolling(): void {
    this.polling?.unsubscribe();
    this.polling = undefined;
  }

  private fetchDiffOnce(): void {
    if (!this.workflowId) {
      return;
    }
    void this.workflowStore
      .getDiff(this.workflowId, this.ignoreWhitespace)
      .then((payload) => {
        this.diffPayload = payload;
        this.renderedFiles = this.buildRenderedDiff(payload);
        this.lastUpdated = new Date();
        this.error = null;
      })
      .catch((err) => {
        this.error = err?.message ?? "Unable to refresh diff.";
      });
  }

  scrollToFile(path: string): void {
    this.diffSections?.find(
      (ref) => ref.nativeElement.dataset["path"] === path,
    )?.nativeElement.scrollIntoView({ behavior: "smooth", block: "start" });
  }

  trackByPath(_: number, file: RenderedDiffFile): string {
    return file.path;
  }

  trackByLine(index: number): number {
    return index;
  }

  private buildRenderedDiff(payload: DiffPayload): RenderedDiffFile[] {
    const files: RenderedDiffFile[] = [];
    const statusMap = new Map(payload.files.map((file) => [file.path, file]));
    const diffLines = payload.unifiedDiff.split("\n");
    let current: RenderedDiffFile | null = null;
    for (const rawLine of diffLines) {
      if (rawLine.startsWith("diff --git ")) {
        const parsedPath = this.extractPathFromDiffHeader(rawLine);
        const status = statusMap.get(parsedPath)?.status ?? "M";
        current = {
          path: parsedPath,
          status,
          lines: [],
        };
        files.push(current);
        continue;
      }
      if (!current) {
        continue;
      }
      const type = this.resolveLineType(rawLine);
      current.lines.push({
        type,
        html: this.renderLine(rawLine, current.path, type),
      });
    }
    return files;
  }

  private extractPathFromDiffHeader(line: string): string {
    const match = /^diff --git a\/(.+?) b\/(.+)$/.exec(line);
    if (match && match[2]) {
      return match[2];
    }
    return line.replace("diff --git", "").trim();
  }

  private resolveLineType(line: string): DiffLineType {
    if (line.startsWith("+") && !line.startsWith("+++")) {
      return "add";
    }
    if (line.startsWith("-") && !line.startsWith("---")) {
      return "del";
    }
    if (line.startsWith("@@")) {
      return "hunk";
    }
    if (line.startsWith("diff --git") || line.startsWith("index ") || line.startsWith("---") || line.startsWith("+++")) {
      return "meta";
    }
    return "context";
  }

  private renderLine(line: string, filePath: string, type: DiffLineType): SafeHtml {
    if (type === "add" || type === "del" || type === "context") {
      const prefix = line.charAt(0);
      const content = line.slice(1);
      const highlighted = this.highlightContent(content, filePath);
      const safePrefix = prefix === " " ? "&nbsp;" : this.escapeHtml(prefix);
      return this.sanitizer.bypassSecurityTrustHtml(
        `<span class="diff-prefix">${safePrefix}</span><span class="diff-code">${highlighted}</span>`,
      );
    }
    return this.sanitizer.bypassSecurityTrustHtml(
      `<span class="diff-code">${this.escapeHtml(line)}</span>`,
    );
  }

  private highlightContent(content: string, filePath: string): string {
    const language = this.detectLanguage(filePath);
    if (language && hljs.getLanguage(language)) {
      try {
        return hljs.highlight(content, { language }).value;
      } catch {
        return this.escapeHtml(content);
      }
    }
    return this.escapeHtml(content);
  }

  private detectLanguage(path: string): string | null {
    const ext = path.split(".").pop()?.toLowerCase();
    if (!ext) {
      return null;
    }
    const mapping: Record<string, string> = {
      ts: "typescript",
      tsx: "typescript",
      js: "javascript",
      jsx: "javascript",
      json: "json",
      html: "html",
      htm: "html",
      css: "css",
      scss: "scss",
      sass: "scss",
      md: "markdown",
      xml: "xml",
      yml: "yaml",
      yaml: "yaml",
      sh: "bash",
      bash: "bash",
      py: "python",
      java: "java",
      go: "go",
      rs: "rust",
      cs: "csharp",
      c: "c",
      h: "c",
      cpp: "cpp",
      hpp: "cpp",
    };
    return mapping[ext] ?? null;
  }

  private escapeHtml(value: string): string {
    return value
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;");
  }
}
