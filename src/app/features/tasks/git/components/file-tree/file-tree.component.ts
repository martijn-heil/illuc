import { CommonModule } from "@angular/common";
import { Component, EventEmitter, Input, Output } from "@angular/core";

export interface FileTreeNode {
    name: string;
    path: string;
    depth: number;
    isFile: boolean;
    status?: string;
    children: FileTreeNode[];
}

@Component({
    selector: "app-file-tree",
    standalone: true,
    imports: [CommonModule],
    templateUrl: "./file-tree.component.html",
    styleUrl: "./file-tree.component.css",
})
export class FileTreeComponent {
    @Input() nodes: FileTreeNode[] = [];
    @Output() selectFile = new EventEmitter<string>();

    private readonly collapsedFolders = new Set<string>();

    trackByNode(_: number, node: FileTreeNode): string {
        return node.path;
    }

    toggleFolder(path: string): void {
        if (this.collapsedFolders.has(path)) {
            this.collapsedFolders.delete(path);
        } else {
            this.collapsedFolders.add(path);
        }
    }

    isCollapsed(path: string): boolean {
        return this.collapsedFolders.has(path);
    }

    handleSelect(path: string): void {
        this.selectFile.emit(path);
    }
}
