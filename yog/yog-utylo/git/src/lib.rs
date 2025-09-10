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

/// Returns the working tree status as a list of [`GitStatusEntry`].
///
/// Both staged (index) and unstaged (worktree) states are captured when present,
/// along with conflict and ignore information.
///
/// Untracked files are included.
/// Ignored files are excluded.
///
/// The output preserves the order produced by `git2`.
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

    let mut out = Vec::new();
    for status_entry in repo.statuses(Some(&mut opts))?.iter() {
        out.push(GitStatusEntry::try_from((repo_root.clone(), &status_entry))?);
    }
    Ok(out)
}

// Using `git restore` because re-implementing its behavior manually would be a bit too much...
// https://stackoverflow.com/a/73759110
pub fn restore(paths: &[&str], branch: Option<&str>) -> color_eyre::Result<()> {
    let mut args = vec!["restore"];
    if let Some(branch) = branch {
        args.push(branch);
    }
    args.extend_from_slice(paths);
    Command::new("git").args(args).exec()?;
    Ok(())
}

#[derive(Debug, Clone)]
pub struct GitStatusEntry {
    pub path: PathBuf,
    pub repo_root: PathBuf,
    pub conflicted: bool,
    pub ignored: bool,
    pub index_state: Option<IndexState>,
    pub worktree_state: Option<WorktreeState>,
}

impl GitStatusEntry {
    pub fn absolute_path(&self) -> PathBuf {
        self.repo_root.join(&self.path)
    }

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

    pub const fn is_new(&self) -> bool {
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

    pub const fn is_new(&self) -> bool {
        matches!(self, Self::New)
    }
}
