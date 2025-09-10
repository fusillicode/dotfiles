#![feature(exit_status_error)]

use std::borrow::Cow;
use std::ops::Deref;
use std::process::Command;

use cmd::CmdExt;
use color_eyre::owo_colors::OwoColorize as _;
use git::GitStatusEntry;
use git::IndexState;
use git::WorktreeState;

/// Interactive CLI tool to clean the working tree by:
///
/// - Deleting newly created or added entries (files or directories)
/// - Restoring modified, renamed, or deleted entries via `git restore`
///
/// Workflow:
/// 1. Collect [`GitStatusEntry`] values via [`crate::git_status::get`].
/// 2. Let the user multi‑select entries via the minimal TUI.
/// 3. Delete new or added entries and run `git restore` (optionally from a user‑supplied branch) for the remaining
///    changed entries.
///
/// The tool is intentionally minimal and suited for quick cleanup and branch‑switching
/// scenarios.
///
/// # Errors
///
/// Returns an error if:
/// - Initializing [`color_eyre`] fails.
/// - Fetching entries via [`crate::git_status::get`] fails.
/// - Presenting the selection UI fails.
/// - Deleting an entry or executing the `git restore` command fails.
fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let args = system::get_args();
    let args: Vec<_> = args.iter().map(String::as_str).collect();

    let git_status_entries = git::get_git_status()?;
    if git_status_entries.is_empty() {
        println!("{}", "working tree clean".bold());
        return Ok(());
    }

    let renderable_entries = git_status_entries.into_iter().map(RenederableGitStatusEntry).collect();

    let Some(selected_entries) = tui::minimal_multi_select::<RenederableGitStatusEntry>(renderable_entries)? else {
        return Ok(());
    };

    restore_entries(selected_entries.iter().map(Deref::deref), args.first().copied())?;

    Ok(())
}

/// Deletes new or added entries (files or directories) and restores changed ones with `git restore`.
///
/// If a `branch` is provided, it is passed immediately after `restore`, so the
/// entries are restored from that branch.
///
/// # Errors
///
/// Returns an error if:
/// - Deleting an entry fails.
/// - Building or executing the `git restore` command fails.
/// - Any underlying I/O operation fails.
fn restore_entries<'a, I>(entries: I, branch: Option<&str>) -> color_eyre::Result<()>
where
    I: Iterator<Item = &'a GitStatusEntry>,
{
    let (new_entries, changed_entries): (Vec<_>, Vec<_>) = entries.partition(|entry| entry.is_new());

    for new_entry in &new_entries {
        if new_entry.path.is_file() || new_entry.path.is_symlink() {
            std::fs::remove_file(&new_entry.path)?;
        } else if new_entry.path.is_dir() {
            std::fs::remove_dir_all(&new_entry.path)?;
        }
        println!("{} {}", "deleted".red().bold(), new_entry.path.display().bold());
    }

    // Exit early in case of no changes to avoid break `git restore` cmd.
    if changed_entries.is_empty() {
        return Ok(());
    }

    let mut args = vec![Cow::Borrowed("restore")];
    if let Some(branch) = branch {
        args.push(Cow::Borrowed(branch));
    }
    let changed_entries_paths = changed_entries
        .iter()
        .map(|ce| ce.path.to_string_lossy())
        .collect::<Vec<_>>();
    args.extend_from_slice(&changed_entries_paths);
    Command::new("git").args(args.iter().map(Cow::as_ref)).exec()?;

    for file_path in changed_entries_paths {
        let from_branch = branch.map(|b| format!(" from {}", b.bold())).unwrap_or_default();
        println!("{} {} from {from_branch}", "restored".yellow().bold(), file_path.bold());
    }
    Ok(())
}

struct RenederableGitStatusEntry(GitStatusEntry);

impl Deref for RenederableGitStatusEntry {
    type Target = GitStatusEntry;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl core::fmt::Display for RenederableGitStatusEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Conflict overrides everything
        if self.conflicted {
            return write!(f, "{} {}", "CC".red().bold(), self.path.display().bold());
        }

        let index_symbol = self
            .index_state
            .as_ref()
            .map(|s| match s {
                IndexState::New => "A",
                IndexState::Modified => "M",
                IndexState::Deleted => "D",
                IndexState::Renamed => "R",
                IndexState::Typechange => "T",
            })
            .unwrap_or(" ");

        let worktree_symbol = self
            .worktree_state
            .as_ref()
            .map(|s| match s {
                WorktreeState::New => "A",
                WorktreeState::Modified => "M",
                WorktreeState::Deleted => "D",
                WorktreeState::Renamed => "R",
                WorktreeState::Typechange => "T",
                WorktreeState::Unreadable => "U",
            })
            .unwrap_or(" ");

        // Optional: mark ignored
        let (idx, wt) = if self.ignored {
            (index_symbol.dimmed().to_string(), worktree_symbol.dimmed().to_string())
        } else {
            (
                style_symbol(index_symbol, self.index_state.is_some(), true),
                style_symbol(worktree_symbol, self.worktree_state.is_some(), false),
            )
        };

        write!(f, "{}{} {}", idx, wt, self.path.display().to_string().bold())
    }
}

fn style_symbol(symbol: &str, present: bool, is_index: bool) -> String {
    use color_eyre::owo_colors::OwoColorize;
    if !present {
        return " ".to_string();
    }
    let styled = match symbol {
        "A" => symbol.green().to_string(),
        "M" => symbol.yellow().to_string(),
        "D" => symbol.red().to_string(),
        "R" => symbol.cyan().to_string(),
        "T" => symbol.magenta().to_string(),
        "U" => symbol.red().bold().to_string(),
        _ => symbol.white().to_string(),
    };
    if is_index {
        styled.bold().to_string()
    } else {
        styled.to_string()
    }
}
