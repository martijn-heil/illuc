import { CommonModule } from "@angular/common";
import {
    Component,
    ChangeDetectorRef,
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
import { Subscription } from "rxjs";
import { DiffMode, DiffPayload } from "../../../task.models";
import { TaskStore } from "../../../task.store";
import {
    FileTreeComponent,
    FileTreeNode,
} from "../file-tree/file-tree.component";

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

const MAX_HIGHLIGHT_CHARS = 200_000;
const MAX_HIGHLIGHT_LINES = 4_000;

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
    selector: "app-task-diff",
    standalone: true,
    imports: [CommonModule, FileTreeComponent],
    templateUrl: "./task-diff.component.html",
    styleUrl: "./task-diff.component.css",
})
export class TaskDiffComponent implements OnChanges, OnDestroy {
    @Input() taskId: string | null = null;
    @Input() baseBranch: string | null = null;
    @ViewChildren("diffSection") diffSections?: QueryList<
        ElementRef<HTMLElement>
    >;

    diffPayload: DiffPayload | null = null;
    renderedFiles: RenderedDiffFile[] = [];
    fileTree: FileTreeNode[] = [];
    lastUpdated: Date | null = null;
    error: string | null = null;
    isLoading = false;
    hasLoaded = false;
    diffMode: DiffMode = "worktree";
    private diffSubscription?: Subscription;
    private diffWatchStop?: () => Promise<void>;

    constructor(
        private readonly taskStore: TaskStore,
        private readonly sanitizer: DomSanitizer,
        private readonly cdr: ChangeDetectorRef,
    ) {}

    ngOnChanges(changes: SimpleChanges): void {
        if (changes["taskId"]) {
            void this.restartDiffWatch();
        }
    }

    ngOnDestroy(): void {
        void this.stopDiffWatch();
    }

    setDiffMode(mode: DiffMode): void {
        if (this.diffMode === mode) {
            return;
        }
        this.diffMode = mode;
        this.diffPayload = null;
        this.renderedFiles = [];
        this.fileTree = [];
        this.error = null;
        this.isLoading = false;
        this.hasLoaded = false;
        void this.restartDiffWatch();
    }

    private async restartDiffWatch(): Promise<void> {
        await this.stopDiffWatch();
        this.diffPayload = null;
        this.renderedFiles = [];
        this.fileTree = [];
        this.error = null;
        this.isLoading = false;
        this.hasLoaded = false;
        if (!this.taskId) {
            return;
        }
        const handle = this.taskStore.watchDiff(this.taskId, this.diffMode);
        this.diffWatchStop = handle.stop;
        this.diffSubscription = handle.state$.subscribe((state) => {
            this.diffPayload = state.payload;
            this.renderedFiles = state.payload
                ? this.buildRenderedDiff(state.payload)
                : [];
            this.fileTree = state.payload
                ? this.buildFileTree(state.payload.files)
                : [];
            this.lastUpdated = state.lastUpdated;
            this.error = state.error;
            this.isLoading = state.isLoading;
            this.hasLoaded = state.hasLoaded;
            this.cdr.detectChanges();
        });
    }

    private async stopDiffWatch(): Promise<void> {
        this.diffSubscription?.unsubscribe();
        this.diffSubscription = undefined;
        if (this.diffWatchStop) {
            await this.diffWatchStop();
            this.diffWatchStop = undefined;
        }
    }

    scrollToFile(path: string): void {
        this.diffSections
            ?.find((ref) => ref.nativeElement.dataset["path"] === path)
            ?.nativeElement.scrollIntoView({
                behavior: "smooth",
                block: "start",
            });
    }

    trackByPath(_: number, file: RenderedDiffFile): string {
        return file.path;
    }

    trackByLine(index: number): number {
        return index;
    }

    private buildRenderedDiff(payload: DiffPayload): RenderedDiffFile[] {
        const files: RenderedDiffFile[] = [];
        const statusMap = new Map(
            payload.files.map((file) => [file.path, file]),
        );
        const diffText = payload.unifiedDiff;
        const diffLines = diffText.split("\n");
        const highlightEnabled =
            diffText.length <= MAX_HIGHLIGHT_CHARS &&
            diffLines.length <= MAX_HIGHLIGHT_LINES;
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
                html: this.renderLine(
                    rawLine,
                    current.path,
                    type,
                    highlightEnabled,
                ),
            });
        }
        return files;
    }

    private buildFileTree(files: DiffPayload["files"]): FileTreeNode[] {
        type BuildNode = {
            name: string;
            path: string;
            isFile: boolean;
            status?: string;
            children: Map<string, BuildNode>;
        };
        const root: BuildNode = {
            name: "",
            path: "",
            isFile: false,
            children: new Map(),
        };
        for (const file of files) {
            const parts = file.path.split("/").filter(Boolean);
            let current = root;
            let currentPath = "";
            for (let index = 0; index < parts.length; index += 1) {
                const part = parts[index];
                currentPath = currentPath ? `${currentPath}/${part}` : part;
                let child = current.children.get(part);
                if (!child) {
                    child = {
                        name: part,
                        path: currentPath,
                        isFile: false,
                        children: new Map(),
                    };
                    current.children.set(part, child);
                }
                if (index === parts.length - 1) {
                    child.isFile = true;
                    child.status = file.status;
                }
                current = child;
            }
        }
        const compressNode = (node: BuildNode): BuildNode => {
            if (node.isFile) {
                return node;
            }
            const children = Array.from(node.children.values()).map(
                compressNode,
            );
            node.children = new Map(
                children.map((child) => [child.name, child]),
            );
            let current = node;
            while (!current.isFile && current.children.size === 1) {
                const onlyChild = Array.from(current.children.values())[0];
                if (onlyChild.isFile) {
                    break;
                }
                current = {
                    name: current.name
                        ? `${current.name}/${onlyChild.name}`
                        : onlyChild.name,
                    path: onlyChild.path,
                    isFile: false,
                    children: onlyChild.children,
                };
            }
            return current;
        };

        const toArray = (node: BuildNode, depth: number): FileTreeNode[] => {
            const children = Array.from(node.children.values()).sort((a, b) => {
                if (a.isFile !== b.isFile) {
                    return a.isFile ? 1 : -1;
                }
                return a.name.localeCompare(b.name);
            });
            return children.map((child) => ({
                name: child.name,
                path: child.path,
                depth,
                isFile: child.isFile,
                status: child.status,
                children: toArray(child, depth + 1),
            }));
        };
        const compressedRoot = compressNode(root);
        return toArray(compressedRoot, 0);
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
        if (
            line.startsWith("diff --git") ||
            line.startsWith("index ") ||
            line.startsWith("---") ||
            line.startsWith("+++")
        ) {
            return "meta";
        }
        return "context";
    }

    private renderLine(
        line: string,
        filePath: string,
        type: DiffLineType,
        highlightEnabled: boolean,
    ): SafeHtml {
        if (type === "add" || type === "del" || type === "context") {
            const prefix = line.charAt(0);
            const content = line.slice(1);
            const highlighted = highlightEnabled
                ? this.highlightContent(content, filePath)
                : this.escapeHtml(content);
            const safePrefix =
                prefix === " " ? "&nbsp;" : this.escapeHtml(prefix);
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
