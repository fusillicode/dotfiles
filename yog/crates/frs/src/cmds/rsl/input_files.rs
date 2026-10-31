//! Select explicit inputs or unstaged Rust files from the enclosing Git worktree.

use std::fs;
use std::io::ErrorKind;
use std::path::Path;
use std::path::PathBuf;

use git2::Repository;
use git2::Status;
use git2::StatusOptions;
use rootcause::prelude::ResultExt;
use rootcause::report;

pub(super) enum InputFiles {
    Explicit(Vec<PathBuf>),
    Unstaged,
}

impl InputFiles {
    pub(super) fn resolve(self) -> rootcause::Result<Vec<PathBuf>> {
        match self {
            Self::Explicit(paths) => Ok(paths),
            Self::Unstaged => unstaged_paths(),
        }
    }
}

fn unstaged_paths() -> rootcause::Result<Vec<PathBuf>> {
    let repository = Repository::discover(".").context("could not discover Git repository for --unstaged")?;
    let root = repository
        .workdir()
        .ok_or_else(|| report!("--unstaged requires a Git working tree"))?;
    let mut options = StatusOptions::new();
    options
        .include_untracked(true)
        .recurse_untracked_dirs(true)
        .include_ignored(false)
        .update_index(false);
    let statuses = repository
        .statuses(Some(&mut options))
        .context("could not read Git status for --unstaged")?;
    let mut paths = Vec::new();

    for entry in &statuses {
        let status = entry.status();
        if status.contains(Status::CONFLICTED)
            || !status.intersects(Status::WT_NEW | Status::WT_MODIFIED | Status::WT_RENAMED | Status::WT_TYPECHANGE)
        {
            continue;
        }

        let Some(delta) = entry.index_to_workdir() else {
            continue;
        };
        let file = delta.new_file();
        let Some(relative) = file.path() else {
            continue;
        };
        if relative.extension().is_none_or(|extension| extension != "rs") {
            continue;
        }

        let path = root.join(relative);
        if is_regular_file(&path)? {
            paths.push(path);
        }
    }

    paths.sort();
    paths.dedup();
    Ok(paths)
}

fn is_regular_file(path: &Path) -> rootcause::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(metadata.file_type().is_file()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(report!("could not inspect unstaged Rust file")
            .attach(format!("path={}", path.display()))
            .attach(error)),
    }
}
