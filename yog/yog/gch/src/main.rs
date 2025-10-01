//! Interactively delete new entries or restore changes in the working tree.
#![feature(exit_status_error)]

use std::ops::Deref;

use color_eyre::owo_colors::OwoColorize as _;
use ytil_git::GitStatusEntry;
use ytil_git::IndexState;
use ytil_git::WorktreeState;

/// Clean the working tree interactively by:
///
/// - Deleting newly created or added entries (files or directories)
/// - Restoring modified, renamed, or deleted entries via `git restore`
///
/// Workflow:
/// 1. Collect [`GitStatusEntry`] values via [`ytil_git::get_status`].
/// 2. Let the user multi‑select entries via the minimal TUI.
/// 3. Delete new or added entries and run `git restore` (optionally from a user‑supplied branch) for the remaining
///    changed entries.
///
/// The tool is intentionally minimal and suited for quick cleanup and branch‑switching
/// scenarios.
///
/// # Usage
///
/// ```bash
/// gch                  # interactive multi-select & clean
/// gch main             # restore changed entries from branch 'main'
/// ```
///
/// # Arguments
///
/// - `[branch]` Optional branch name; if provided changed entries are restored from it.
///
/// # Errors
/// In case:
/// - Initializing [`color_eyre`] fails.
/// - Fetching entries via [`ytil_git::get_status`] fails.
/// - Presenting the selection UI fails.
/// - Deleting an entry or executing the `git restore` command fails.
fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let args = ytil_system::get_args();
    let args: Vec<_> = args.iter().map(String::as_str).collect();

    let git_status_entries = ytil_git::get_status()?;
    if git_status_entries.is_empty() {
        println!("{}", "working tree clean".bold());
        return Ok(());
    }

    let renderable_entries = git_status_entries.into_iter().map(RenederableGitStatusEntry).collect();

    let Some(selected_entries) = ytil_tui::minimal_multi_select::<RenederableGitStatusEntry>(renderable_entries)?
    else {
        println!("\n\n{}", "nothing done".bold());
        return Ok(());
    };

    println!();
    restore_entries(selected_entries.iter().map(Deref::deref), args.first().copied())?;

    Ok(())
}

/// Deletes new or added entries (files or directories) and restores changed ones with `git restore`.
///
/// If a `branch` is provided, it is passed immediately after `restore`, so the
/// entries are restored from that branch.
///
/// # Errors
/// In case:
/// - Removing a file or directory for a new entry fails.
/// - Building or executing the `git restore` command fails.
fn restore_entries<'a, I>(entries: I, branch: Option<&str>) -> color_eyre::Result<()>
where
    I: Iterator<Item = &'a GitStatusEntry>,
{
    let (new_entries, changed_entries): (Vec<_>, Vec<_>) = entries.partition(|entry| entry.is_new());

    for new_entry in &new_entries {
        let absolute_path = new_entry.absolute_path();
        if absolute_path.is_file() || absolute_path.is_symlink() {
            std::fs::remove_file(&absolute_path)?;
        } else if absolute_path.is_dir() {
            std::fs::remove_dir_all(&absolute_path)?;
        }
        println!("{} {}", "deleted".red().bold(), new_entry.path.display().bold());
    }

    // Exit early in case of no changes to avoid break `git restore` cmd.
    if changed_entries.is_empty() {
        return Ok(());
    }

    let changed_entries_paths = changed_entries
        .iter()
        .map(|changed_entry| changed_entry.absolute_path().to_string_lossy().into_owned())
        .collect::<Vec<_>>();

    ytil_git::restore(
        &changed_entries_paths.iter().map(String::as_str).collect::<Vec<_>>(),
        branch,
    )?;

    for changed_entry in changed_entries {
        let from_branch = branch.map(|b| format!(" from {}", b.bold())).unwrap_or_default();
        println!(
            "{} {}{from_branch}",
            "restored".yellow().bold(),
            changed_entry.path.display().bold()
        );
    }
    Ok(())
}

/// Wrapper newtype to implement `Display` for [`GitStatusEntry`].
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

        let index_symbol = self.index_state.as_ref().map_or_else(
            || " ".to_string(),
            |s| match s {
                IndexState::New => "A".green().bold().to_string(),
                IndexState::Modified => "M".yellow().bold().to_string(),
                IndexState::Deleted => "D".red().bold().to_string(),
                IndexState::Renamed => "R".cyan().bold().to_string(),
                IndexState::Typechange => "T".magenta().bold().to_string(),
            },
        );

        let worktree_symbol = self.worktree_state.as_ref().map_or_else(
            || " ".to_string(),
            |s| match s {
                WorktreeState::New => "A".green().bold().to_string(),
                WorktreeState::Modified => "M".yellow().bold().to_string(),
                WorktreeState::Deleted => "D".red().bold().to_string(),
                WorktreeState::Renamed => "R".cyan().bold().to_string(),
                WorktreeState::Typechange => "T".magenta().bold().to_string(),
                WorktreeState::Unreadable => "U".red().bold().to_string(),
            },
        );

        // Ignored marks as dimmed
        let (index_symbol, worktree_symbol) = if self.ignored {
            (index_symbol.dimmed().to_string(), worktree_symbol.dimmed().to_string())
        } else {
            (index_symbol, worktree_symbol)
        };

        write!(f, "{}{} {}", index_symbol, worktree_symbol, self.path.display())
    }
}
