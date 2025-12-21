export type WorkflowStatus =
  | "CREATING_WORKTREE"
  | "READY"
  | "RUNNING"
  | "WAITING_FOR_INPUT"
  | "COMPLETED"
  | "FAILED"
  | "STOPPED"
  | "DISCARDED";

export interface WorkflowSummary {
  workflowId: string;
  title: string;
  status: WorkflowStatus;
  createdAt: string;
  startedAt?: string | null;
  endedAt?: string | null;
  worktreePath: string;
  branchName: string;
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

export interface DiffPayload {
  workflowId: string;
  files: DiffFile[];
  unifiedDiff: string;
}

export interface TerminalOutputEvent {
  workflowId: string;
  data: string;
}

export interface TerminalExitEvent {
  workflowId: string;
  exitCode: number;
}
