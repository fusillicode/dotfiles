use std::ops::Deref;
use std::path::Path;

use color_eyre::owo_colors::OwoColorize;
use gch::GitOperation;
use gch::RenderableGitStatusEntry;
use strum::IntoEnumIterator;
use ytil_git::GitStatusEntry;

fn main() -> color_eyre::Result<()> {
    let args = ytil_system::get_args();
    let args: Vec<_> = args.iter().map(String::as_str).collect();

    let git_status_entries = ytil_git::get_status()?;
    if git_status_entries.is_empty() {
        println!("{}", "working tree clean".bold());
        return Ok(());
    }

    let renderable_entries = git_status_entries.into_iter().map(RenderableGitStatusEntry).collect();

    let Some(selected_entries) = ytil_tui::minimal_multi_select::<RenderableGitStatusEntry>(renderable_entries)? else {
        println!("\n\n{}", "nothing done".bold());
        return Ok(());
    };

    let Some(selected_op) = ytil_tui::minimal_select::<GitOperation>(GitOperation::iter().collect())? else {
        println!("\n\n{}", "nothing done".bold());
        return Ok(());
    };

    let selected_entries = selected_entries.iter().map(Deref::deref).collect::<Vec<_>>();
    match selected_op {
        GitOperation::Restore => restore_entries(&selected_entries, args.first().copied())?,
        GitOperation::Stage => stage_entries(&selected_entries)?,
    }

    println!();

    Ok(())
}

/// Delete new entries and restore changed ones with `git restore`.
///
/// If a `branch` is provided, it is passed so changed entries are restored from that branch.
///
/// # Arguments
/// - `entries` Selected Git entries.
/// - `branch` Optional branch name to restore from.
///
/// # Returns
/// `Ok(())` after processing; early returns after deleting if no changed entries remain.
///
/// # Errors
/// - Removing a file or directory for a new entry fails.
/// - Building or executing the `git restore` command fails.
fn restore_entries(entries: &[&GitStatusEntry], branch: Option<&str>) -> color_eyre::Result<()> {
    let (new_entries, changed_entries): (Vec<&GitStatusEntry>, Vec<&GitStatusEntry>) =
        entries.iter().partition(|entry| entry.is_new());

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

/// Stage the provided entries (equivalent to `git add` on each path).
///
/// # Arguments
/// - `entries` Selected Git entries.
///
/// # Errors
/// - Opening the repository fails.
/// - Adding any path to the index fails.
fn stage_entries(entries: &[&GitStatusEntry]) -> color_eyre::Result<()> {
    let mut repo = ytil_git::get_repo(Path::new("."))?;
    ytil_git::add_to_index(&mut repo, entries.iter().map(|entry| entry.path.as_path()))?;
    for entry in entries {
        println!("{} {}", "staged".green().bold(), entry.path.display().bold());
    }
    Ok(())
}
