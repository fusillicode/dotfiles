//! Stage or discard selected Git changes interactively
//!
//! Presents a compact TUI to multi‑select working tree entries and apply a bulk
//! operation (stage or discard) with colorized progress output. Canceling any
//! prompt safely results in no changes.
//!
//! # Arguments
//! - `<branch>` Optional branch used as blob source during restore in Discard; if omitted,
//!   `git restore` falls back to index / HEAD.
//!
//! # Exit Codes
//! - `0` Success (includes user cancellations performing no changes).
//! - Non‑zero: bubbled I/O, subprocess, or git operation failure (reported via `color_eyre`).
//!
//! # Errors
//! - Status enumeration fails.
//! - User interaction (selection prompts) fails.
//! - File / directory removal for new entries fails.
//! - Unstaging new index entries via [`ytil_git::unstage`] fails.
//! - Restore command construction / execution fails.
//! - Opening repository or adding paths to index fails.
//!
//! # Rationale
//! - Delegates semantics to porcelain (`git restore`, `git add`) to inherit nuanced Git behavior.
//! - Minimal two‑prompt UX optimizes rapid iterative staging / discarding.
use std::ops::Deref;
use std::path::Path;

use color_eyre::owo_colors::OwoColorize;
use gch::Op;
use gch::RenderableGitStatusEntry;
use strum::IntoEnumIterator;
use ytil_git::GitStatusEntry;

/// Provide an interactive Git helper to stage or discard selected changes.
///
/// # Returns
/// `Ok(())` if the program completes without I/O or subprocess errors.
///
/// # Errors
/// - Any failure bubbling from status retrieval, user interaction, or Git operations.
fn main() -> color_eyre::Result<()> {
    let args = ytil_system::get_args();
    let args: Vec<_> = args.iter().map(String::as_str).collect();

    let git_status_entries = ytil_git::get_status()?;
    if git_status_entries.is_empty() {
        println!("{}", "Working tree clean".bold());
        return Ok(());
    }

    let renderable_entries = git_status_entries.into_iter().map(RenderableGitStatusEntry).collect();

    let Some(selected_entries) = ytil_tui::minimal_multi_select::<RenderableGitStatusEntry>(renderable_entries)? else {
        println!("\n\n{}", "Nothing done".bold());
        return Ok(());
    };

    let Some(selected_op) = ytil_tui::minimal_select::<Op>(Op::iter().collect())? else {
        println!("\n\n{}", "Nothing done".bold());
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
/// - Building or executing the underlying `git restore` command fails.
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
            changed_entry.path.display().bold()
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
/// - Opening the repository fails.
/// - Adding any path to the index fails.
fn add_entries(entries: &[&GitStatusEntry]) -> color_eyre::Result<()> {
    let mut repo = ytil_git::get_repo(Path::new("."))?;
    ytil_git::add_to_index(&mut repo, entries.iter().map(|entry| entry.path.as_path()))?;
    for entry in entries {
        println!("{} {}", "Added".green().bold(), entry.path.display().bold());
    }
    Ok(())
}
