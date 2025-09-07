#![feature(exit_status_error)]

use std::borrow::Cow;
use std::process::Command;

use color_eyre::owo_colors::OwoColorize as _;
use utils::cmd::CmdExt;

use crate::git::GitStatusEntry;

mod git;

/// Interactive CLI tools to clean the working tree by:
///
/// - Deleting newly created files
/// - Restoring modified, renamed, or deleted ones via `git restore`
///
/// Workflow:
/// 1. Collect [`GitStatusEntry`] values via [`git::get_git_status_entries`].
/// 2. Let the user multi‑select entries with skim (see [`utils::sk`]).
/// 3. Delete new or added files and run `git restore` (optionally from a user‑supplied branch) for the remaining
///    changed entries.
///
/// The tool is intentionally minimal and suited for quick cleanup and branch‑switching
/// scenarios.
///
/// # Errors
///
/// Returns an error if:
/// - Initializing [`color_eyre`] fails.
/// - Fetching entries via [`git::get_git_status_entries`] fails.
/// - Building skim options or running [`utils::sk::get_items`] fails.
/// - Deleting a file or executing the `git restore` command fails.
fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let args = utils::system::get_args();
    let args: Vec<_> = args.iter().map(String::as_str).collect();

    let selected_entries = select_git_status_entries()?;

    let branch = args.first().copied();
    restore_files(&selected_entries, branch)?;

    Ok(())
}

/// Collects git status entries and lets the user multi‑select them.
///
/// Uses skim with multi‑selection and a hidden preview window.
///
/// # Errors
///
/// Returns an error if:
/// - Reading entries via [`git::get_git_status_entries`] fails.
/// - Building skim options fails.
/// - Interactive selection via [`utils::sk::get_items`] fails.
fn select_git_status_entries() -> color_eyre::Result<Vec<GitStatusEntry>> {
    let git_status_entries = crate::git::get_git_status_entries()?;

    let mut opts_builder = utils::sk::default_opts_builder();
    let base_opts = utils::sk::base_sk_opts(&mut opts_builder)
        .multi(true)
        .preview_window("hidden".to_string());

    Ok(utils::sk::get_items(git_status_entries, Some(base_opts.build()?))?
        .into_iter()
        .collect::<Vec<_>>())
}

/// Deletes new or added files and restores changed ones with `git restore`.
///
/// If a `branch` is provided, it is passed immediately after `restore`, so the
/// files are restored from that branch.
///
/// # Errors
///
/// Returns an error if:
/// - Deleting a file fails.
/// - Building or executing the `git restore` command fails.
/// - Any underlying I/O operation fails.
fn restore_files(entries: &[GitStatusEntry], branch: Option<&str>) -> color_eyre::Result<()> {
    let (new_entries, changed_entries): (Vec<_>, Vec<_>) = entries.iter().partition(|entry| match entry {
        GitStatusEntry::New(_) | GitStatusEntry::Added(_) => true,
        GitStatusEntry::Modified(_) | GitStatusEntry::Renamed(_) | GitStatusEntry::Deleted(_) => false,
    });

    for new_entry in new_entries {
        let file_path = new_entry.file_path();
        std::fs::remove_file(file_path)?;
        println!("{} {}", "- deleted".red().bold(), file_path.display().bold());
    }

    if changed_entries.is_empty() {
        println!("{}", "no changes".bold());
        return Ok(());
    }

    let mut args = vec![Cow::Borrowed("restore")];
    if let Some(branch) = branch {
        args.push(Cow::Borrowed(branch));
    }
    let changed_entries_paths = changed_entries
        .iter()
        .map(|ce| ce.file_path().to_string_lossy())
        .collect::<Vec<_>>();
    args.extend_from_slice(&changed_entries_paths);
    Command::new("git").args(args.iter().map(Cow::as_ref)).exec()?;

    for file_path in changed_entries_paths {
        let from_branch = branch.map(|b| format!(" from {}", b.bold())).unwrap_or_default();
        println!(
            "{} {} from {from_branch}",
            "< restored".yellow().bold(),
            file_path.bold()
        );
    }
    Ok(())
}
