//! Interactively delete new entries or restore changes in the working tree.
//!
//! Presents a multi-select UI; selected entries are:
//! - Deleted if newly created / added.
//! - Restored (`git restore`) if modified / renamed / deleted / type-changed, optionally from a provided branch.
//!
//! This binary now focuses solely on cleanup; staging moved to `gcha`.
#![feature(exit_status_error)]

use std::ops::Deref;

use color_eyre::owo_colors::OwoColorize as _;
use ytil_git::GitStatusEntry;

/// Entry point: interactive selection then delete (new) / restore (changed) operations.
///
/// # Errors
/// In case:
/// - Initializing `color_eyre` fails.
/// - Collecting status entries or running selection UI fails.
/// - Deleting a file/directory or executing the restore command fails.
fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let args = ytil_system::get_args();
    let args: Vec<_> = args.iter().map(String::as_str).collect();

    gch::apply_on_selected_git_status_entries(|selected_entries| {
        restore_entries(selected_entries.iter().map(Deref::deref), args.first().copied())
    })?;

    Ok(())
}

/// Delete new entries and restore changed ones with `git restore`.
///
/// If a `branch` is provided, it is passed so changed entries are restored from that branch.
///
/// # Arguments
/// * `entries` - Iterator of selected status entries.
/// * `branch` - Optional branch name to restore from.
///
/// # Returns
/// `Ok(())` after processing; early returns after deleting if no changed entries remain.
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
