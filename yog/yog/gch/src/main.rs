//! Stage or discard selected Git changes interactively
//!
//! Presents a compact TUI to multi‑select working tree entries and apply a bulk
//! operation (stage or discard) with colorized progress output. Canceling any
//! prompt safely results in no changes.
//!
//! # Arguments
//! - `<branch>` Optional branch used as blob source during restore in Discard; if omitted, `git restore` falls back to
//!   index / HEAD.
//!
//! # Usage
//! ```bash
//! gch # select changes -> choose Add or Discard
//! gch main # use 'main' as blob source when discarding
//! ```
//!
//! # Exit Codes
//! - `0` Success (includes user cancellations performing no changes).
//! - Non‑zero: bubbled I/O, subprocess, or git operation failure (reported via `color_eyre`).
//!
//! # Errors
//! - Status enumeration via [`ytil_git::get_status`] fails.
//! - User interaction (selection prompts via [`ytil_tui::minimal_multi_select`] and [`ytil_tui::minimal_select`])
//!   fails.
//! - File / directory removal for new entries fails.
//! - Unstaging new index entries via [`ytil_git::unstage`] fails.
//! - Restore command construction / execution via [`ytil_git::restore`] fails.
//! - Opening repository via [`ytil_git::get_repo`] or adding paths to index via [`ytil_git::add_to_index`] fails.
//!
//! # Rationale
//! - Delegates semantics to porcelain (`git restore`, `git add`) to inherit nuanced Git behavior.
//! - Minimal two‑prompt UX optimizes rapid iterative staging / discarding.

use core::fmt::Display;
use std::ops::Deref;
use std::path::Path;

use color_eyre::owo_colors::OwoColorize;
use strum::EnumIter;
use strum::IntoEnumIterator;
use ytil_git::GitStatusEntry;
use ytil_git::IndexState;
use ytil_git::WorktreeState;

/// The actual `main` inner logic.
///
/// # Errors
/// - Status enumeration via [`ytil_git::get_status`] fails.
/// - User interaction (selection prompts via [`ytil_tui::minimal_multi_select`] and [`ytil_tui::minimal_select`])
///   fails.
/// - File / directory removal for new entries fails.
/// - Unstaging new index entries via [`ytil_git::unstage`] fails.
/// - Restore command construction / execution via [`ytil_git::restore`] fails.
/// - Opening repository via [`ytil_git::get_repo`] or adding paths to index via [`ytil_git::add_to_index`] fails.
fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let args = ytil_system::get_args();
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

/// Delete newly created paths then restore modified paths (optionally from a branch)
///
/// Performs a two‑phase operation over the provided `entries`:
/// 1) Physically removes any paths that are newly added (worktree and/or index). For paths that were staged as new,
///    their repo‑relative paths are collected and subsequently unstaged via [`ytil_git::unstage`], ensuring only the
///    index is touched (no accidental content resurrection).
/// 2) Invokes [`ytil_git::restore`] for any remaining changed (non‑new) entries, optionally specifying `branch` so
///    contents are restored from that branch rather than the index / HEAD.
///
/// Early exit: if after deleting new entries there are no remaining changed entries, the
/// restore phase is skipped.
///
/// # Arguments
/// - `entries` Slice of status entries selected by the user (borrowed, not consumed).
/// - `branch` Optional branch name; when `Some`, restore reads blobs from that branch.
///
/// # Returns
/// `Ok(())` if all deletions / unstaging / restore operations succeed.
///
/// # Errors
/// - Removing a file or directory for a new entry fails (I/O error from `std::fs`).
/// - Unstaging staged new entries via [`ytil_git::unstage`] fails.
/// - Building or executing the underlying `git restore` command via [`ytil_git::restore`] fails.
///
/// # Rationale
/// Using the porcelain `git restore` preserves nuanced semantics (e.g. respect for sparse
/// checkout, renames) without re‑implementing them atop libgit2.
///
/// # Future Work
/// - Detect & report partial failures (continue deletion on best‑effort then aggregate errors).
/// - Parallelize deletions if ever shown to be a bottleneck (likely unnecessary for typical counts).
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
        println!("{} {}", "Discarded".red().bold(), new_entry.path.display().bold());
        if new_entry.is_new_in_index() {
            new_entries_in_index.push(absolute_path.display().to_string());
        }
    }
    // Use repo-relative paths for unstaging so we *only* touch the index.
    ytil_git::unstage(&new_entries_in_index.iter().map(String::as_str).collect::<Vec<_>>())?;

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
            "Restored".yellow().bold(),
            changed_entry.path.display().white().bold()
        );
    }
    Ok(())
}

/// Add the provided entries to the Git index (equivalent to `git add` on each path).
///
/// # Arguments
/// - `entries` Selected Git entries.
///
/// # Errors
/// - Opening the repository via [`ytil_git::get_repo`] fails.
/// - Adding any path to the index via [`ytil_git::add_to_index`] fails.
fn add_entries(entries: &[&GitStatusEntry]) -> color_eyre::Result<()> {
    let mut repo = ytil_git::get_repo(Path::new("."))?;
    ytil_git::add_to_index(&mut repo, entries.iter().map(|entry| entry.path.as_path()))?;
    for entry in entries {
        println!("{} {}", "Added".green().bold(), entry.path.display().bold());
    }
    Ok(())
}

/// Newtype wrapper adding colored [`Display`] for a [`ytil_git::GitStatusEntry`].
///
/// Renders two status columns (index + worktree) plus the path, dimming ignored entries
/// and prioritizing conflict markers.
///
/// # Examples
/// ```no_run
/// # fn show(e: &RenderableGitStatusEntry) {
/// println!("{e}");
/// # }
/// ```
///
/// # Rationale
/// Needed to implement [`Display`] without modifying an external type (orphan rule).
///
/// # Performance
/// Only constructs small colored string fragments per render.
///
/// # Future Work
/// - Provide a structured render method (symbols + path) for alternative UIs.
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
        let str_repr = match self {
            Self::Discard => format!("{}", "Discard".red().bold()),
            Self::Add => "Add".green().bold().to_string(),
        };
        write!(f, "{str_repr}")
    }
}
