//! macOS quarantine extended-attribute cleanup.

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::thread;

use rootcause::report;

/// The result of cleaning one repo's quarantine metadata.
pub struct RepoCleanup {
    /// Repo that was inspected.
    pub repo: PathBuf,
    /// Cleanup failures encountered while inspecting the repo.
    pub failures: Vec<rootcause::Report>,
}

/// Removes quarantine metadata from every repo while skipping nested repos and symbolic links.
pub fn clean(repos: &[PathBuf], jobs: usize) -> Vec<RepoCleanup> {
    repos.iter().map(|repo| clean_repo(repo, repos, jobs)).collect()
}

fn clean_repo(repo: &Path, repos: &[PathBuf], jobs: usize) -> RepoCleanup {
    let mut failures = Vec::new();
    let mut workers = std::collections::VecDeque::new();
    workers.push_back(thread::spawn({
        let repo = repo.to_path_buf();
        move || clean_path(&repo)
    }));
    let mut directories = match std::fs::read_dir(repo) {
        Ok(entries) => vec![(repo.to_path_buf(), entries)],
        Err(error) => {
            failures.push(
                report!("quarantine traversal failed").attach(format!("directory={} error={error}", repo.display())),
            );
            Vec::new()
        }
    };

    while let Some((directory, entries)) = directories.last_mut() {
        let directory = directory.clone();
        let entry = entries.next();
        let Some(entry) = entry else {
            directories.pop();
            continue;
        };

        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                failures.push(
                    report!("quarantine traversal failed")
                        .attach(format!("directory={} error={error}", directory.display())),
                );
                continue;
            }
        };

        let path = entry.path();
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(error) => {
                failures
                    .push(report!("could not inspect path").attach(format!("path={} error={error}", path.display())));
                continue;
            }
        };
        if file_type.is_symlink() || repos.iter().any(|nested| nested != repo && path.starts_with(nested)) {
            continue;
        }

        let child_entries = if file_type.is_dir() {
            match std::fs::read_dir(&path) {
                Ok(entries) => Some(entries),
                Err(error) => {
                    failures.push(
                        report!("quarantine traversal failed")
                            .attach(format!("directory={} error={error}", path.display())),
                    );
                    None
                }
            }
        } else {
            None
        };
        if workers.len() >= jobs
            && let Some(worker) = workers.pop_front()
            && let Some(failure) = collect_cleanup(worker)
        {
            failures.push(failure);
        }
        if let Some(entries) = child_entries {
            directories.push((path.clone(), entries));
        }
        workers.push_back(thread::spawn(move || clean_path(&path)));
    }

    for worker in workers {
        if let Some(failure) = collect_cleanup(worker) {
            failures.push(failure);
        }
    }

    RepoCleanup {
        repo: repo.to_path_buf(),
        failures,
    }
}

fn clean_path(path: &Path) -> rootcause::Result<()> {
    let output = Command::new("xattr").arg(path).output().map_err(|error| {
        report!("could not inspect quarantine metadata").attach(format!("path={} error={error}", path.display()))
    })?;
    if !output.status.success() {
        return Err(report!("could not inspect quarantine metadata").attach(format!("path={}", path.display())));
    }

    let quarantine = String::from_utf8_lossy(&output.stdout)
        .lines()
        .any(|attribute| attribute == "com.apple.quarantine");
    if !quarantine {
        return Ok(());
    }

    let status = Command::new("xattr")
        .args(["-d", "com.apple.quarantine"])
        .arg(path)
        .status()
        .map_err(|error| {
            report!("could not remove quarantine metadata").attach(format!("path={} error={error}", path.display()))
        })?;

    if status.success() {
        return Ok(());
    }
    Err(report!("could not remove quarantine metadata").attach(format!("path={}", path.display())))
}

fn collect_cleanup(cleanup: thread::JoinHandle<rootcause::Result<()>>) -> Option<rootcause::Report> {
    match cleanup.join() {
        Ok(Ok(())) => None,
        Ok(Err(failure)) => Some(failure),
        Err(_) => Some(report!("quarantine cleanup worker panicked")),
    }
}
