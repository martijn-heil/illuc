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
