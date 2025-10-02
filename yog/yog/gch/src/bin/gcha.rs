//! Interactively stage (add to index) selected git status entries.
//!
//! Presents a multi-select UI over current status entries and stages the chosen paths.
#![feature(exit_status_error)]

use std::ops::Deref;
use std::path::Path;

use ytil_git::GitStatusEntry;

/// Entry point: interactive selection then staging of chosen entries.
///
/// # Errors
/// - Initializing `color_eyre` fails.
/// - Collecting git status or running the selection UI fails.
/// - Opening the repository or updating the index fails.
fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    gch::apply_on_selected_git_status_entries(|selected_entries| {
        add_entries_to_index(selected_entries.iter().map(Deref::deref))
    })?;

    Ok(())
}

/// Stage the provided entries (equivalent to `git add` on each path).
///
/// # Arguments
/// - `entries` Iterator of selected [`ytil_git::GitStatusEntry`] references.
///
/// # Errors
/// - Opening the repository fails.
/// - Adding any path to the index fails.
fn add_entries_to_index<'a, I>(entries: I) -> color_eyre::Result<()>
where
    I: Iterator<Item = &'a GitStatusEntry>,
{
    let mut repo = ytil_git::get_repo(Path::new("."))?;
    ytil_git::add_to_index(&mut repo, entries.into_iter().map(|entry| entry.path.as_path()))?;
    Ok(())
}
