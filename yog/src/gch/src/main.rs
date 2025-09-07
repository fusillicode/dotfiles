#![feature(exit_status_error)]

use std::process::Command;

use color_eyre::owo_colors::OwoColorize as _;
use utils::cmd::CmdExt;

use crate::git::GitStatusEntry;

mod git;

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let args = utils::system::get_args();
    let args: Vec<_> = args.iter().map(String::as_str).collect();
    let branch = args.first().copied();

    let selected_entries = select_git_status_entries()?;

    let selected_files_paths = &selected_entries
        .iter()
        .map(GitStatusEntry::file_path)
        .collect::<Vec<_>>();

    restore_files(
        &selected_files_paths.iter().map(String::as_str).collect::<Vec<_>>(),
        branch,
    )?;

    Ok(())
}

fn select_git_status_entries() -> color_eyre::Result<Vec<GitStatusEntry>> {
    let git_status_entries = crate::git::git_status()?;

    let mut opts_builder = utils::sk::default_opts_builder();
    let base_opts = utils::sk::base_sk_opts(&mut opts_builder).multi(true);

    Ok(utils::sk::get_items(git_status_entries, Some(base_opts.build()?))?
        .into_iter()
        .collect::<Vec<_>>())
}

fn restore_files(files: &[&str], branch: Option<&str>) -> color_eyre::Result<()> {
    if files.is_empty() {
        return Ok(());
    }
    let mut args = vec!["restore"];
    if let Some(branch) = branch {
        args.push(branch);
    }
    args.extend_from_slice(files);
    Command::new("git").args(args).exec()?;
    for file in files {
        let from_branch = branch.map(|b| format!(" from {}", b.bold())).unwrap_or_default();
        println!("{} {} {from_branch}", "<".yellow().bold(), file.bold());
    }
    Ok(())
}
