#![feature(exit_status_error)]

use std::borrow::Cow;
use std::process::Command;

use cmd::CmdExt;
use color_eyre::owo_colors::OwoColorize as _;

use crate::git_status::GitStatusEntry;

mod git_status;

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

    let Some(selected_entries) = tui::minimal_multi_select::<GitStatusEntry>(crate::git_status::get()?)? else {
        return Ok(());
    };
    restore_entries(&selected_entries, args.first().copied())?;

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
fn restore_entries(entries: &[GitStatusEntry], branch: Option<&str>) -> color_eyre::Result<()> {
    let (new_entries, changed_entries): (Vec<_>, Vec<_>) = entries.iter().partition(|entry| match entry {
        GitStatusEntry::New(_) | GitStatusEntry::Added(_) => true,
        GitStatusEntry::Modified(_) | GitStatusEntry::Renamed(_) | GitStatusEntry::Deleted(_) => false,
    });

    for new_entry in &new_entries {
        let entry_path = new_entry.path();
        if entry_path.is_file() || entry_path.is_symlink() {
            std::fs::remove_file(entry_path)?;
        } else if entry_path.is_dir() {
            std::fs::remove_dir_all(entry_path)?;
        }
        println!("{} {}", "deleted".red().bold(), entry_path.display().bold());
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
        .map(|ce| ce.path().to_string_lossy())
        .collect::<Vec<_>>();
    args.extend_from_slice(&changed_entries_paths);
    Command::new("git").args(args.iter().map(Cow::as_ref)).exec()?;

    for file_path in changed_entries_paths {
        let from_branch = branch.map(|b| format!(" from {}", b.bold())).unwrap_or_default();
        println!("{} {} from {from_branch}", "restored".yellow().bold(), file_path.bold());
    }
    Ok(())
}
