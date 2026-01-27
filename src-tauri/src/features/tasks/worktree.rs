use crate::error::Result;
use std::path::{Path, PathBuf};

pub fn managed_worktree_root(repo_root: &Path) -> Result<PathBuf> {
    let illuc_dir = repo_root.join(".illuc");
    let worktree_dir = illuc_dir.join("worktrees");
    if !worktree_dir.exists() {
        std::fs::create_dir_all(&worktree_dir)?;
    }
    Ok(worktree_dir)
}

pub fn clean_branch_name(branch: &str) -> String {
    branch
        .trim()
        .strip_prefix("refs/heads/")
        .unwrap_or(branch.trim())
        .to_string()
}

pub fn format_title_from_branch(branch: &str) -> String {
    let slug = branch.split('/').last().unwrap_or(branch);
    let (task_id, label) = extract_task_and_label(slug);
    if let Some(task) = task_id {
        format!("[{}] {}", task, label)
    } else {
        label
    }
}

fn extract_task_and_label(slug: &str) -> (Option<String>, String) {
    let mut range: Option<(usize, usize)> = None;
    let mut digits = String::new();
    let mut iter = slug.char_indices().peekable();
    while let Some((start_idx, ch)) = iter.next() {
        if ch.is_ascii_digit() {
            digits.clear();
            digits.push(ch);
            let mut end_idx = start_idx + ch.len_utf8();
            while let Some(&(next_idx, next_ch)) = iter.peek() {
                if next_ch.is_ascii_digit() {
                    digits.push(next_ch);
                    end_idx = next_idx + next_ch.len_utf8();
                    iter.next();
                } else {
                    break;
                }
            }
            if digits.len() >= 3 {
                range = Some((start_idx, end_idx));
                break;
            }
        }
    }

    let mut remainder = slug.to_string();
    let task_id = if let Some((start, end)) = range {
        let task = remainder[start..end].to_string();
        remainder.replace_range(start..end, " ");
        Some(task)
    } else {
        None
    };

    let cleaned = remainder
        .replace(&['-', '_'][..], " ")
        .split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => {
                    first.to_uppercase().collect::<String>()
                        + chars.as_str().to_lowercase().as_str()
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ");

    let label = if cleaned.is_empty() {
        slug.replace(&['/', '-', '_'][..], " ")
            .split_whitespace()
            .map(|word| {
                let mut chars = word.chars();
                match chars.next() {
                    Some(first) => {
                        first.to_uppercase().collect::<String>()
                            + chars.as_str().to_lowercase().as_str()
                    }
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        cleaned
    };

    (task_id, label.trim().to_string())
}
