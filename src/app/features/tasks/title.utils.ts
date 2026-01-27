export interface TitleParts {
    taskId: string | null;
    label: string;
}

export function parseTitleParts(title: string | null | undefined): TitleParts {
    if (!title) {
        return { taskId: null, label: "" };
    }
    const trimmed = title.trim();
    const match = /^\[(\d+)\]\s*(.+)$/i.exec(trimmed);
    if (match) {
        return {
            taskId: match[1],
            label: match[2],
        };
    }
    return {
        taskId: null,
        label: trimmed,
    };
}

export function deriveTitleFromBranch(branchName: string): string {
    const slug = branchName.split("/").pop() ?? branchName;
    const taskMatch = slug.match(/(\d{3,})/);
    const taskId = taskMatch ? taskMatch[1] : null;
    let remainder = slug;
    if (taskMatch) {
        remainder = remainder.replace(taskMatch[1], "");
    }
    remainder = remainder.replace(/[-_]+/g, " ").replace(/\s+/g, " ").trim();
    if (!remainder) {
        remainder = branchName.replace(/[-_/]+/g, " ").trim();
    }
    const human = remainder
        ? remainder
              .split(" ")
              .filter(Boolean)
              .map(
                  (word) =>
                      word.charAt(0).toUpperCase() +
                      word.slice(1).toLowerCase(),
              )
              .join(" ")
        : branchName;
    return taskId ? `[${taskId}] ${human}` : human;
}
