//! Shared interactive selection & rendering utilities for `gch` binaries.
//!
//! Provides a common flow:
//! 1. Collect [`ytil_git::GitStatusEntry`] via [`ytil_git::get_status`].
//! 2. Wrap them in [`RenederableGitStatusEntry`] for colored display (similar to porcelain output).
//! 3. Present a multi‑select TUI (`ytil_tui::minimal_multi_select`).
//! 4. Invoke a caller callback on the chosen subset.
//!
//! Used by binaries:
//! - `gcha` (stage selected entries)
//! - `gchr` (restore / delete selected entries)
use std::ops::Deref;

use color_eyre::owo_colors::OwoColorize;
use ytil_git::GitStatusEntry;
use ytil_git::IndexState;
use ytil_git::WorktreeState;

/// Run interactive git status selection then invoke a callback on chosen entries.
///
/// # Arguments
/// * `apply_fn` - Callback receiving a slice of selected [`RenederableGitStatusEntry`].
///
/// # Returns
/// `Ok(())` when:
/// - Working tree is clean (prints a message).
/// - User aborts selection (prints a message).
/// - Callback completes successfully.
///
/// Propagates underlying errors otherwise.
///
/// # Errors
/// In case:
/// - Retrieving status via [`ytil_git::get_status`] fails.
/// - Running `ytil_tui::minimal_multi_select` fails.
/// - The callback `apply_fn` returns an error.
pub fn apply_on_selected_git_status_entries(
    apply_fn: impl Fn(&[RenederableGitStatusEntry]) -> color_eyre::Result<()>,
) -> color_eyre::Result<()> {
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
    apply_fn(&selected_entries)?;

    Ok(())
}

/// Newtype wrapper adding colored [`core::fmt::Display`] for a [`ytil_git::GitStatusEntry`].
///
/// Renders two status columns (index + worktree) plus the path, dimming ignored entries
/// and prioritizing conflict markers.
///
/// # Examples
/// ```no_run
/// # fn show(e: &gch::RenederableGitStatusEntry) {
/// println!("{e}");
/// # }
/// ```
///
/// # Rationale
/// Needed to implement [`std::fmt::Display`] without modifying an external type (orphan rule).
///
/// # Performance
/// Only constructs small colored string fragments per render.
///
/// # Future Work
/// - Provide a structured render method (symbols + path) for alternative UIs.
pub struct RenederableGitStatusEntry(GitStatusEntry);

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
