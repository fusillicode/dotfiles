//! Lightweight Git helper utilities built on top of [`git2`].
//!
//! This module provides small wrappers around common read / write repository
//! interactions used by the surrounding tooling:
//! - Repository discovery and root path resolution ([`get_repo`], [`get_repo_root`])
//! - Current branch inspection and simple branch creation / switching
//! - Working tree status collection as structured data ([`get_status`] returning [`GitStatusEntry`])
//! - Branch enumeration with last commit timestamps ([`get_branches`])
//! - Convenience helpers such as filtering redundant remote branches ([`remove_redundant_remotes`])
//!
//! Some commands (e.g. the special case in [`switch_branch`] for `-` and [`restore`])
//! deliberately defer to the system `git` binary instead of re‑implementing more
//! involved porcelain semantics.

use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use chrono::DateTime;
use chrono::Utc;
use cmd::CmdExt as _;
use color_eyre::eyre::bail;
use color_eyre::eyre::eyre;
use git2::Repository;
use git2::Status;
use git2::StatusEntry;
use git2::StatusOptions;

/// Returns the [`Repository`] containing `path`.
///
/// Starts discovery from `path` and walks up parent directories using
/// [`git2::Repository::discover`].
///
/// # Errors
///
/// Returns an error:
/// - If the repository cannot be discovered starting from `path` (i.e. `path` is not inside a Git repository).
pub fn get_repo(path: &Path) -> color_eyre::Result<Repository> {
    Ok(Repository::discover(path)?)
}

/// Returns the absolute path to the working tree root of `repo`.
///
/// The path is derived from [`Repository::commondir`].
/// Any trailing `.git` component (for non‑bare repositories) is removed.
/// For bare repositories the returned path is the repository directory itself.
pub fn get_repo_root(repo: &Repository) -> PathBuf {
    repo.commondir()
        .components()
        .filter(|c| c.as_os_str() != ".git")
        .collect()
}

/// Returns the name of the current branch (e.g. `main`).
///
/// # Errors
///
/// Returns an error if:
/// - The repository cannot be discovered.
/// - `HEAD` is detached.
/// - The branch name is not valid UTF-8.
pub fn get_current_branch() -> color_eyre::Result<String> {
    let repo_path = Path::new(".");
    let repo = get_repo(repo_path)?;

    if repo.head_detached()? {
        bail!("detached head for git {}", repo_path.display())
    }

    repo.head()?
        .shorthand()
        .map(str::to_string)
        .ok_or_else(|| eyre!("shorthand is not valid UTF-8"))
}

/// Creates a new local branch pointing to the current `HEAD` commit.
///
/// Does not switch to the new branch.
///
/// # Errors
///
/// Returns an error if:
/// - The repository cannot be discovered.
/// - `HEAD` cannot be resolved to a commit.
/// - The branch already exists.
pub fn create_branch(branch_name: &str) -> color_eyre::Result<()> {
    let repo_path = Path::new(".");
    let repo = get_repo(repo_path)?;

    let commit = repo.head()?.peel_to_commit()?;
    repo.branch(branch_name, &commit, false)?;

    Ok(())
}

/// Switches `HEAD` to `branch_name` (or detaches if it resolves to a commit).
///
/// Special case: if `branch_name` is `-` the system `git` is invoked with
/// `git switch -` to reuse Git's built‑in previous branch logic (not currently
/// modeled directly in [`git2`]).
///
/// # Errors
///
/// Returns an error if:
/// - The repository cannot be discovered.
/// - Name resolution fails.
/// - Checkout fails.
pub fn switch_branch(branch_name: &str) -> color_eyre::Result<()> {
    // TODO: understand if there is a straightforward way with git2
    if branch_name == "-" {
        Command::new("git").args(["switch", branch_name]).exec()?;
        return Ok(());
    }

    let repo = get_repo(Path::new("."))?;

    // Find the branch reference
    let (object, reference) = repo.revparse_ext(branch_name)?;

    // Checkout the branch head
    repo.checkout_tree(&object, None)?;

    // Set HEAD to point to the new branch reference
    match reference {
        Some(reference) => repo.set_head(reference.name().ok_or_else(|| eyre!("reference name is not UTF-8"))?)?,
        None => repo.set_head_detached(object.id())?,
    }

    Ok(())
}

/// Returns the working tree status as a list of [`GitStatusEntry`].
///
/// Both staged (index) and unstaged (worktree) states are captured when present,
/// along with conflict and ignore information. Untracked files are included,
/// ignored files are excluded. Order reflects the iteration order from
/// [`git2`].
///
/// # Errors
///
/// Returns an error if:
/// - The repository cannot be discovered.
/// - Reading statuses fails.
/// - A status entry is missing a path (required to build a [`GitStatusEntry`]).
pub fn get_status() -> color_eyre::Result<Vec<GitStatusEntry>> {
    let repo = get_repo(Path::new("."))?;
    let repo_root = get_repo_root(&repo);

    let mut opts = StatusOptions::default();
    opts.include_untracked(true);
    opts.include_ignored(false);

    let mut out = vec![];
    for status_entry in repo.statuses(Some(&mut opts))?.iter() {
        out.push(GitStatusEntry::try_from((repo_root.clone(), &status_entry))?);
    }
    Ok(out)
}

/// Restores one or more paths from the index or an optional `branch`.
///
/// Equivalent to invoking `git restore [<branch>] <paths...>`.
/// The system `git` binary is used instead of re‑implementing restore semantics
/// to avoid accidental complexity – see <https://stackoverflow.com/a/73759110>).
///
/// # Errors
///
/// Returns an error if:
/// - Executing the underlying `git restore` command fails.
pub fn restore(paths: &[&str], branch: Option<&str>) -> color_eyre::Result<()> {
    let mut args = vec!["restore"];
    if let Some(branch) = branch {
        args.push(branch);
    }
    args.extend_from_slice(paths);
    Command::new("git").args(args).exec()?;
    Ok(())
}

/// Returns all local and remote [`Branch`]es sorted by last committer date
/// (most recent first).
///
/// # Errors
///
/// Returns an error if:
/// - The repository cannot be discovered.
/// - Enumerating branches fails.
/// - A branch name is not valid UTF-8.
/// - Resolving the branch tip commit fails.
/// - Converting the committer timestamp into a [`DateTime`] fails.
pub fn get_branches() -> color_eyre::Result<Vec<Branch>> {
    let repo = get_repo(Path::new("."))?;
    let mut out = vec![];

    for branch_res in repo.branches(None)? {
        let (raw_branch, branch_type) = branch_res?;

        let branch_name = raw_branch
            .name()?
            .ok_or_else(|| eyre!("branch name is not valid UTF-8"))?;
        let commit_time = raw_branch.get().peel_to_commit()?.committer().when();
        let committer_date_time = DateTime::from_timestamp(commit_time.seconds(), 0)
            .ok_or_else(|| eyre!("cannot create DateTime<Utc> from seconds {}", commit_time.seconds()))?;

        let branch = match branch_type {
            git2::BranchType::Local => Branch::Local {
                name: branch_name.to_string(),
                committer_date_time,
            },
            git2::BranchType::Remote => Branch::Remote {
                name: branch_name.to_string(),
                committer_date_time,
            },
        };

        out.push(branch);
    }

    out.sort_by(|a, b| b.committer_date_time().cmp(a.committer_date_time()));

    Ok(out)
}

/// Removes remote branches that have a corresponding local branch of the same
/// shortened name.
///
/// A remote branch is considered redundant if its name after the first `/`
/// (e.g. `origin/feature-x` -> `feature-x`) matches a local branch name.
///
/// After this function returns, each remaining [`Branch::Remote`] has no local
/// counterpart with the same short name.
pub fn remove_redundant_remotes(branches: &mut Vec<Branch>) {
    let mut local_names = HashSet::with_capacity(branches.len());
    for branch in branches.iter() {
        if let Branch::Local { name, .. } = branch {
            local_names.insert(name.clone());
        }
    }

    branches.retain(|b| match b {
        Branch::Local { .. } => true,
        Branch::Remote { name, .. } => {
            let short = name.split_once('/').map_or(name.as_str(), |(_, rest)| rest);
            !local_names.contains(short)
        }
    });
}

/// Local or remote branch with metadata about the last commit.
#[derive(Clone, Debug)]
#[cfg_attr(test, derive(PartialEq, Eq))]
pub enum Branch {
    /// Local branch (under `refs/heads/`).
    Local {
        /// The name of the branch (without refs/heads/ or refs/remotes/ prefix).
        name: String,
        /// The date and time when the last commit was made.
        committer_date_time: DateTime<Utc>,
    },
    /// Remote tracking branch (under `refs/remotes/`).
    Remote {
        /// The name of the branch (without refs/heads/ or refs/remotes/ prefix).
        name: String,
        /// The date and time when the last commit was made.
        committer_date_time: DateTime<Utc>,
    },
}

impl Branch {
    /// Returns the branch name (no `refs/` prefix).
    pub fn name(&self) -> &str {
        match self {
            Self::Local { name, .. } | Self::Remote { name, .. } => name,
        }
    }

    /// Returns the timestamp of the last commit on this branch.
    pub const fn committer_date_time(&self) -> &DateTime<Utc> {
        match self {
            Self::Local {
                committer_date_time, ..
            }
            | Self::Remote {
                committer_date_time, ..
            } => committer_date_time,
        }
    }
}

/// Single entry representing the status of a path in the working tree.
///
/// Combines both index (staged) and worktree (unstaged) information along with
/// conflict / ignore state. Helper methods expose derived semantics.
#[derive(Debug, Clone)]
pub struct GitStatusEntry {
    /// Path relative to the repository root.
    pub path: PathBuf,
    /// Absolute repository root path used to compute [`GitStatusEntry::absolute_path`].
    pub repo_root: PathBuf,
    /// `true` if the path is in a conflict state.
    pub conflicted: bool,
    /// `true` if the path is ignored.
    pub ignored: bool,
    /// Staged (index) status, if any.
    pub index_state: Option<IndexState>,
    /// Unstaged (worktree) status, if any.
    pub worktree_state: Option<WorktreeState>,
}

impl GitStatusEntry {
    /// Returns the absolute path of the entry relative to the repository root.
    pub fn absolute_path(&self) -> PathBuf {
        self.repo_root.join(&self.path)
    }

    /// Returns `true` if the entry is newly added (in index or worktree).
    pub fn is_new(&self) -> bool {
        if self.index_state.as_ref().is_some_and(IndexState::is_new)
            || self.worktree_state.as_ref().is_some_and(WorktreeState::is_new)
        {
            return true;
        }
        false
    }
}

impl TryFrom<(PathBuf, &StatusEntry<'_>)> for GitStatusEntry {
    type Error = color_eyre::Report;

    fn try_from((repo_root, value): (PathBuf, &StatusEntry<'_>)) -> Result<Self, Self::Error> {
        let status = value.status();
        let path = value
            .path()
            .map(PathBuf::from)
            .ok_or_else(|| eyre!("cannot build GitStatusEntry, missing path in StatusEntry"))?;

        Ok(Self {
            path,
            repo_root,
            conflicted: status.contains(Status::CONFLICTED),
            ignored: status.contains(Status::IGNORED),
            index_state: IndexState::new(&status),
            worktree_state: WorktreeState::new(&status),
        })
    }
}

/// Staged (index) status for a path.
#[derive(Debug, Clone)]
pub enum IndexState {
    /// Path added to the index.
    New,
    /// Path modified in the index.
    Modified,
    /// Path deleted from the index.
    Deleted,
    /// Path renamed in the index.
    Renamed,
    /// File type changed in the index (e.g. regular file -> symlink).
    Typechange,
}

impl IndexState {
    /// Creates an [`IndexState`] from a combined status bit‑set.
    pub fn new(status: &Status) -> Option<Self> {
        [
            (Status::INDEX_NEW, Self::New),
            (Status::INDEX_MODIFIED, Self::Modified),
            (Status::INDEX_DELETED, Self::Deleted),
            (Status::INDEX_RENAMED, Self::Renamed),
            (Status::INDEX_TYPECHANGE, Self::Typechange),
        ]
        .iter()
        .find(|(flag, _)| status.contains(*flag))
        .map(|(_, v)| v)
        .cloned()
    }

    /// Returns `true` if this represents a newly added path.
    pub const fn is_new(&self) -> bool {
        matches!(self, Self::New)
    }
}

/// Unstaged (worktree) status for a path.
#[derive(Debug, Clone)]
pub enum WorktreeState {
    /// Path newly created in worktree.
    New,
    /// Path contents modified in worktree.
    Modified,
    /// Path deleted in worktree.
    Deleted,
    /// Path renamed in worktree.
    Renamed,
    /// File type changed in worktree.
    Typechange,
    /// Path unreadable (permissions or other I/O issues).
    Unreadable,
}

impl WorktreeState {
    /// Creates a [`WorktreeState`] from a combined status bit‑set.
    pub fn new(status: &Status) -> Option<Self> {
        [
            (Status::WT_NEW, Self::New),
            (Status::WT_MODIFIED, Self::Modified),
            (Status::WT_DELETED, Self::Deleted),
            (Status::WT_RENAMED, Self::Renamed),
            (Status::WT_TYPECHANGE, Self::Typechange),
            (Status::WT_UNREADABLE, Self::Unreadable),
        ]
        .iter()
        .find(|(flag, _)| status.contains(*flag))
        .map(|(_, v)| v)
        .cloned()
    }

    /// Returns `true` if this represents a newly added path.
    pub const fn is_new(&self) -> bool {
        matches!(self, Self::New)
    }
}
