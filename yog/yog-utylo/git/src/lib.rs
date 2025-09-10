use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

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
/// Returns an error if:
/// - The repository cannot be discovered starting from `path`.
/// - `path` is not inside a Git repository.
pub fn get_repo(path: &Path) -> color_eyre::Result<Repository> {
    Ok(Repository::discover(path)?)
}

/// Returns the absolute path to the repository working tree root that contains `path`.
///
/// Derives the path from [`Repository::commondir`] and removes any trailing `.git` component if present.
/// For bare repositories (no working tree) this simply returns the repository directory itself.
///
/// # Errors
///
/// Returns an error if:
/// - The repository cannot be discovered starting from `path`.
/// - `path` is not inside a Git repository.
pub fn get_repo_root(path: &Path) -> color_eyre::Result<PathBuf> {
    let repo = get_repo(path)?;
    Ok(repo
        .commondir()
        .components()
        .filter(|c| c.as_os_str() != ".git")
        .collect())
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
/// Special case: if `branch_name` is `-` it shell‑invokes `git switch -` to reuse Git's previous branch logic.
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

pub fn get_git_status() -> color_eyre::Result<Vec<GitStatusEntry>> {
    let repo = get_repo(Path::new("."))?;
    let mut out = Vec::new();
    let mut opts = StatusOptions::default();
    opts.include_ignored(false);
    for status_entry in repo.statuses(Some(&mut opts))?.iter() {
        out.push(GitStatusEntry::try_from(&status_entry)?);
    }
    Ok(out)
}

#[derive(Debug, Clone)]
pub struct GitStatusEntry {
    pub path: PathBuf,
    pub conflicted: bool,
    pub ignored: bool,
    pub index_state: Option<IndexState>,
    pub worktree_state: Option<WorktreeState>,
}

impl GitStatusEntry {
    pub fn is_new(&self) -> bool {
        if self.index_state.as_ref().is_some_and(IndexState::is_new)
            || self.worktree_state.as_ref().is_some_and(WorktreeState::is_new)
        {
            return true;
        }
        false
    }
}

impl TryFrom<&StatusEntry<'_>> for GitStatusEntry {
    type Error = color_eyre::Report;

    fn try_from(value: &StatusEntry<'_>) -> Result<Self, Self::Error> {
        let status = value.status();
        let path = value
            .path()
            .map(PathBuf::from)
            .ok_or_else(|| eyre!("cannot build GitStatusEntry, missing path in StatusEntry"))?;

        Ok(Self {
            path,
            conflicted: status.contains(Status::CONFLICTED),
            ignored: status.contains(Status::IGNORED),
            index_state: IndexState::new(&status),
            worktree_state: WorktreeState::new(&status),
        })
    }
}

#[derive(Debug, Clone)]
pub enum IndexState {
    New,
    Modified,
    Deleted,
    Renamed,
    Typechange,
}

impl IndexState {
    pub fn new(status: &Status) -> Option<Self> {
        [
            (Status::INDEX_NEW, IndexState::New),
            (Status::INDEX_MODIFIED, IndexState::Modified),
            (Status::INDEX_DELETED, IndexState::Deleted),
            (Status::INDEX_RENAMED, IndexState::Renamed),
            (Status::INDEX_TYPECHANGE, IndexState::Typechange),
        ]
        .iter()
        .find(|(flag, _)| status.contains(*flag))
        .map(|(_, v)| v)
        .cloned()
    }

    pub fn is_new(&self) -> bool {
        matches!(self, Self::New)
    }
}

#[derive(Debug, Clone)]
pub enum WorktreeState {
    New,
    Modified,
    Deleted,
    Renamed,
    Typechange,
    Unreadable,
}

impl WorktreeState {
    pub fn new(status: &Status) -> Option<Self> {
        [
            (Status::WT_NEW, WorktreeState::New),
            (Status::WT_MODIFIED, WorktreeState::Modified),
            (Status::WT_DELETED, WorktreeState::Deleted),
            (Status::WT_RENAMED, WorktreeState::Renamed),
            (Status::WT_TYPECHANGE, WorktreeState::Typechange),
            (Status::WT_UNREADABLE, WorktreeState::Unreadable),
        ]
        .iter()
        .find(|(flag, _)| status.contains(*flag))
        .map(|(_, v)| v)
        .cloned()
    }

    pub fn is_new(&self) -> bool {
        matches!(self, Self::New)
    }
}
