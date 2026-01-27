export type TaskStatus =
    | "CREATING_WORKTREE"
    | "IDLE"
    | "AWAITING_APPROVAL"
    | "WORKING"
    | "COMPLETED"
    | "FAILED"
    | "STOPPED"
    | "DISCARDED";

export enum AgentKind {
    Codex = "codex",
    Copilot = "copilot",
}

export interface TaskSummary {
    taskId: string;
    title: string;
    status: TaskStatus;
    createdAt: string;
    startedAt?: string | null;
    endedAt?: string | null;
    worktreePath: string;
    branchName: string;
    baseBranch: string;
    baseRepoPath: string;
    baseCommit: string;
    exitCode?: number | null;
}

export interface BaseRepoInfo {
    path: string;
    canonicalPath: string;
    currentBranch: string;
    head: string;
}

export interface DiffFile {
    path: string;
    status: string;
}

export type DiffMode = "worktree" | "branch";

export interface DiffPayload {
    taskId: string;
    files: DiffFile[];
    unifiedDiff: string;
}

export interface DiffChangedEvent {
    taskId: string;
}

export type TerminalKind = "agent" | "worktree";

export interface TerminalOutputEvent {
    taskId: string;
    data: string;
    kind: TerminalKind;
}

export interface TerminalExitEvent {
    taskId: string;
    exitCode: number;
    kind: TerminalKind;
}
