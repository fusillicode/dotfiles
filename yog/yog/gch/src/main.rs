//! Stage or discard selected Git changes interactively.
//!
//! # Errors
//! - Git operations or user interaction fails.

use core::fmt::Display;
use std::ops::Deref;
use std::path::Path;

use color_eyre::owo_colors::OwoColorize;
use strum::EnumIter;
use strum::IntoEnumIterator;
use ytil_git::GitStatusEntry;
use ytil_git::IndexState;
use ytil_git::WorktreeState;
use ytil_sys::cli::Args;

/// Newtype wrapper adding colored [`Display`] for a [`ytil_git::GitStatusEntry`].
///
/// Renders two status columns (index + worktree) plus the path.
pub struct RenderableGitStatusEntry(pub GitStatusEntry);

impl Deref for RenderableGitStatusEntry {
    type Target = GitStatusEntry;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Display for RenderableGitStatusEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Conflict overrides everything
        if self.conflicted {
            return write!(f, "{} {}", "CC".red().bold(), self.path.display());
        }

        // Write index symbol directly to formatter, avoiding intermediate String allocations.
        write_index_symbol(f, self.index_state.as_ref(), self.ignored)?;
        write_worktree_symbol(f, self.worktree_state.as_ref(), self.ignored)?;

        write!(f, " {}", self.path.display())
    }
}

/// High-level Git working tree/index operations exposed by the UI.
#[derive(EnumIter)]
pub enum Op {
    /// Add path contents to the index similar to `git add <path>`.
    Add,
    /// Discard changes in the worktree and/or reset the index for a path
    /// similar in spirit to `git restore` / `git checkout -- <path>`.
    Discard,
}

impl Display for Op {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Discard => write!(f, "{}", "Discard".red().bold()),
            Self::Add => write!(f, "{}", "Add".green().bold()),
        }
    }
}

/// Delete newly created paths then restore modified paths.
///
/// # Errors
/// - File removal, unstaging, or restore command fails.
fn restore_entries(entries: &[&GitStatusEntry], branch: Option<&str>) -> color_eyre::Result<()> {
    // Avoid creating &&GitStatusEntry by copying the slice of &GitStatusEntry directly.
    let (new_entries, changed_entries): (Vec<&GitStatusEntry>, Vec<&GitStatusEntry>) =
        entries.iter().copied().partition(|entry| entry.is_new());

    let mut new_entries_in_index = vec![];
    for new_entry in &new_entries {
        let absolute_path = new_entry.absolute_path();
        if absolute_path.is_file() || absolute_path.is_symlink() {
            std::fs::remove_file(&absolute_path)?;
        } else if absolute_path.is_dir() {
            std::fs::remove_dir_all(&absolute_path)?;
        }
        println!("{} {}", "Discarded".red().bold(), new_entry.path.display());
        if new_entry.is_new_in_index() {
            new_entries_in_index.push(absolute_path.display().to_string());
        }
    }
    // Use repo-relative paths for unstaging so we *only* touch the index.
    ytil_git::unstage(new_entries_in_index.iter().map(String::as_str))?;

    // Exit early in case of no changes to avoid break `git restore` cmd.
    if changed_entries.is_empty() {
        return Ok(());
    }

    let changed_entries_paths = changed_entries
        .iter()
        .map(|changed_entry| changed_entry.absolute_path().to_string_lossy().into_owned());

    ytil_git::restore(changed_entries_paths, branch)?;

    for changed_entry in changed_entries {
        let from_branch = branch.map(|b| format!(" from {b}")).unwrap_or_default();
        println!(
            "{} {}{from_branch}",
            "Restored".yellow().bold(),
            changed_entry.path.display()
        );
    }
    Ok(())
}

/// Add the provided entries to the Git index (equivalent to `git add` on each path).
///
/// # Errors
/// - Opening the repository via [`ytil_git::repo::discover`] fails.
/// - Adding any path to the index via [`ytil_git::add_to_index`] fails.
fn add_entries(entries: &[&GitStatusEntry]) -> color_eyre::Result<()> {
    let mut repo = ytil_git::repo::discover(Path::new("."))?;
    ytil_git::add_to_index(&mut repo, entries.iter().map(|entry| entry.path.as_path()))?;
    for entry in entries {
        println!("{} {}", "Added".green().bold(), entry.path.display());
    }
    Ok(())
}

/// Write an index state symbol directly to the formatter, avoiding `.to_string()` allocations.
fn write_index_symbol(f: &mut std::fmt::Formatter<'_>, state: Option<&IndexState>, dimmed: bool) -> std::fmt::Result {
    match state {
        None => write!(f, " "),
        Some(IndexState::New) if dimmed => write!(f, "{}", "A".green().bold().dimmed()),
        Some(IndexState::New) => write!(f, "{}", "A".green().bold()),
        Some(IndexState::Modified) if dimmed => write!(f, "{}", "M".yellow().bold().dimmed()),
        Some(IndexState::Modified) => write!(f, "{}", "M".yellow().bold()),
        Some(IndexState::Deleted) if dimmed => write!(f, "{}", "D".red().bold().dimmed()),
        Some(IndexState::Deleted) => write!(f, "{}", "D".red().bold()),
        Some(IndexState::Renamed) if dimmed => write!(f, "{}", "R".cyan().bold().dimmed()),
        Some(IndexState::Renamed) => write!(f, "{}", "R".cyan().bold()),
        Some(IndexState::Typechange) if dimmed => write!(f, "{}", "T".magenta().bold().dimmed()),
        Some(IndexState::Typechange) => write!(f, "{}", "T".magenta().bold()),
    }
}

/// Write a worktree state symbol directly to the formatter, avoiding `.to_string()` allocations.
fn write_worktree_symbol(
    f: &mut std::fmt::Formatter<'_>,
    state: Option<&WorktreeState>,
    dimmed: bool,
) -> std::fmt::Result {
    match state {
        None => write!(f, " "),
        Some(WorktreeState::New) if dimmed => write!(f, "{}", "A".green().bold().dimmed()),
        Some(WorktreeState::New) => write!(f, "{}", "A".green().bold()),
        Some(WorktreeState::Modified) if dimmed => write!(f, "{}", "M".yellow().bold().dimmed()),
        Some(WorktreeState::Modified) => write!(f, "{}", "M".yellow().bold()),
        Some(WorktreeState::Deleted) if dimmed => write!(f, "{}", "D".red().bold().dimmed()),
        Some(WorktreeState::Deleted) => write!(f, "{}", "D".red().bold()),
        Some(WorktreeState::Renamed) if dimmed => write!(f, "{}", "R".cyan().bold().dimmed()),
        Some(WorktreeState::Renamed) => write!(f, "{}", "R".cyan().bold()),
        Some(WorktreeState::Typechange) if dimmed => write!(f, "{}", "T".magenta().bold().dimmed()),
        Some(WorktreeState::Typechange) => write!(f, "{}", "T".magenta().bold()),
        Some(WorktreeState::Unreadable) if dimmed => write!(f, "{}", "U".red().bold().dimmed()),
        Some(WorktreeState::Unreadable) => write!(f, "{}", "U".red().bold()),
    }
}

/// Stage or discard selected Git changes interactively.
///
/// # Errors
/// - Status enumeration via [`ytil_git::get_status`] fails.
/// - User interaction (selection prompts via [`ytil_tui::minimal_multi_select`] and [`ytil_tui::minimal_select`])
///   fails.
/// - File / directory removal for new entries fails.
/// - Unstaging new index entries via [`ytil_git::unstage`] fails.
/// - Restore command construction / execution via [`ytil_git::restore`] fails.
/// - Opening repository via [`ytil_git::repo::discover`] or adding paths to index via [`ytil_git::add_to_index`] fails.
fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let args = ytil_sys::cli::get();
    if args.has_help() {
        println!("{}", include_str!("../help.txt"));
        return Ok(());
    }
    let args: Vec<_> = args.iter().map(String::as_str).collect();

    let git_status_entries = ytil_git::get_status()?;
    if git_status_entries.is_empty() {
        println!("Working tree clean");
        return Ok(());
    }

    let renderable_entries = git_status_entries.into_iter().map(RenderableGitStatusEntry).collect();

    let Some(selected_entries) = ytil_tui::minimal_multi_select::<RenderableGitStatusEntry>(renderable_entries)? else {
        println!("\n\nNo entries selected");
        return Ok(());
    };

    let Some(selected_op) = ytil_tui::minimal_select::<Op>(Op::iter().collect())? else {
        println!("\n\nNothing operation selected");
        return Ok(());
    };

    let selected_entries = selected_entries.iter().map(Deref::deref).collect::<Vec<_>>();
    match selected_op {
        Op::Discard => restore_entries(&selected_entries, args.first().copied())?,
        Op::Add => add_entries(&selected_entries)?,
    }

    Ok(())
}
