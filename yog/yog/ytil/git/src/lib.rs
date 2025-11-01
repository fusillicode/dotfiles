//! Provide lightweight Git helpers atop [`git2`] plus selective fallbacks to the system `git` binary.
//!
//! Wrap common operations (repo discovery, root resolution, status enumeration, branch listing,
//! targeted fetch, branch switching, restore) in focused functions returning structured data
//! (`GitStatusEntry`, `Branch`). Some semantics (previous branch with `switch -`, restore) defer to
//! the porcelain CLI to avoid re‑implementing complex behavior.

use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use chrono::DateTime;
use chrono::Utc;
use color_eyre::eyre::bail;
use color_eyre::eyre::eyre;
use git2::Cred;
use git2::IntoCString;
use git2::RemoteCallbacks;
use git2::Repository;
use git2::Status;
use git2::StatusEntry;
use git2::StatusOptions;
use ytil_cmd::CmdExt as _;

/// Discover the Git repository containing `path`.
///
/// Wrapper over [`git2::Repository::discover`]. Walks parent directories upward until a repo
/// root is found.
///
/// # Arguments
/// - `path` Starting filesystem path (file or directory) inside the repo.
///
/// # Returns
/// Open [`Repository`].
///
/// # Errors
/// - Not inside a Git repository (discovery fails).
///
/// # Future Work
/// - Accept an option to disallow bare repositories.
pub fn get_repo(path: &Path) -> color_eyre::Result<Repository> {
    Ok(Repository::discover(path)?)
}

/// Absolute working tree root path for repository
///
/// Derived from [`Repository::commondir`] with any trailing `.git` removed (non‑bare repos).
/// Bare repositories return their directory path unchanged.
pub fn get_repo_root(repo: &Repository) -> PathBuf {
    repo.commondir()
        .components()
        .filter(|c| c.as_os_str() != ".git")
        .collect()
}

/// Get current branch name (fails if HEAD detached).
///
/// # Returns
/// Branch short name (e.g. `main`).
///
/// # Errors
/// - Repository discovery fails.
/// - HEAD is detached.
/// - Branch shorthand not valid UTF-8.
///
/// # Future Work
/// - Provide enum distinguishing detached state instead of error.
pub fn get_current_branch() -> color_eyre::Result<String> {
    let repo_path = Path::new(".");
    let repo = get_repo(repo_path)?;

    if repo.head_detached()? {
        bail!("detached head | repo_path={}", repo_path.display())
    }

    repo.head()?
        .shorthand()
        .map(str::to_string)
        .ok_or_else(|| eyre!("branch shorthand invalid utf-8 | repo_path={}", repo_path.display()))
}

/// Create a new local branch at current HEAD (no checkout).
///
/// Branch starts at the commit pointed to by `HEAD`; caller remains on the original branch.
///
/// # Arguments
/// - `branch_name` Name of branch to create (must not already exist).
///
/// # Returns
/// [`Result::Ok`] (()) if creation succeeds.
///
/// # Errors
/// - Repository discovery fails.
/// - Resolving `HEAD` to a commit fails.
/// - Branch already exists.
///
/// # Future Work
/// - Optionally force (move) existing branch with a flag.
/// - Support creating tracking configuration in one step.
pub fn create_branch(branch_name: &str) -> color_eyre::Result<()> {
    let repo = get_repo(Path::new("."))?;

    let commit = repo.head()?.peel_to_commit()?;
    repo.branch(branch_name, &commit, false)?;

    Ok(())
}

/// Checkout a branch or detach HEAD; supports previous branch shorthand and branch creation via guessing.
///
/// Defers to `git switch --guess` to leverage porcelain semantics, which can create a new branch
/// if the name is ambiguous and matches an existing remote branch.
///
/// # Arguments
/// - `branch_name` Branch name or revision (use `-` for prior branch).
///
/// # Errors
/// - Spawning or executing the `git switch` command fails.
///
/// # Future Work
/// - Expose progress callbacks for large checkouts.
pub fn switch_branch(branch_name: &str) -> color_eyre::Result<()> {
    Command::new("git").args(["switch", branch_name, "--guess"]).exec()?;
    Ok(())
}

/// Enumerate combined staged + unstaged status entries.
///
/// Builds [`GitStatusEntry`] values capturing index + worktree states plus conflict / ignore
/// flags. Includes untracked, excludes ignored. Order matches libgit2 iteration order.
///
/// # Returns
/// Vector of status entries (may be empty if clean working tree).
///
/// # Errors
/// - Repository discovery fails.
/// - Reading statuses fails.
/// - A status entry omits a path (required to construct a [`GitStatusEntry`]).
///
/// # Rationale
/// Centralizes translation from libgit2 status bitflags into a friendlier struct with helper
/// methods used by higher‑level commands.
///
/// # Future Work
/// - Add option to include ignored entries.
/// - Parameterize repo path instead of implicit current directory.
/// - Expose performance metrics (count, timing) for diagnostics.
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

/// Restore one or more paths from index or optional branch,
///
/// Delegates to porcelain `git restore` rather than approximating behavior with libgit2.
/// If `branch` is provided its tree is the source; otherwise the index / HEAD is used.
///
/// # Arguments
/// - `paths` Absolute or relative paths to restore. Empty slice = no‑op.
/// - `branch` Optional branch (or commit-ish) acting as the source of truth.
///
/// # Returns
/// [`Result::Ok`] (()) if the command spawns and completes successfully (zero status).
///
/// # Errors
/// - Spawning or executing the `git restore` process fails.
///
/// # Rationale
/// Porcelain subcommand encapsulates nuanced restore semantics (rename detection, pathspec
/// interpretation) that would be complex and error‑prone to replicate directly.
///
/// # Future Work
/// - Support partial restore when command fails mid‑batch by iterating per path.
/// - Add dry‑run flag to preview intended operations.
pub fn restore(paths: &[&str], branch: Option<&str>) -> color_eyre::Result<()> {
    let mut args = vec!["restore"];
    if let Some(branch) = branch {
        args.push(branch);
    }
    args.extend_from_slice(paths);
    Command::new("git").args(args).exec()?;
    Ok(())
}

/// Unstage specific paths without touching working tree contents.
///
/// Thin wrapper over `git restore --staged <paths...>` which only affects the index
/// (inverse of `git add`). Unlike using libgit2 `reset_default`, this avoids
/// resurrecting deleted files whose blobs no longer exist on disk.
///
/// # Arguments
/// - `paths` Repo‑relative paths currently staged (any state) to unstage. Empty slice = no‑op.
///
/// # Returns
/// [`Result::Ok`] (()) if command spawns and exits successfully (zero status).
///
/// # Errors
/// - Spawning or executing the `git restore --staged` command fails.
///
/// # Rationale
/// Defers to porcelain for correctness (handles intent, pathspec edge cases) instead of
/// manually editing the index via libgit2 which exhibited unintended side effects during
/// experimentation.
///
/// # Future Work
/// - Optionally fall back to libgit2 for environments lacking a `git` binary.
/// - Capture command stderr and surface as richer context on failure.
pub fn unstage(paths: &[&str]) -> color_eyre::Result<()> {
    if paths.is_empty() {
        return Ok(());
    }
    // Use porcelain `git restore --staged` which modifies only the index (opposite of `git add`).
    // This avoids resurrecting deleted files (observed when using libgit2 `reset_default`).
    Command::new("git").args(["restore", "--staged"]).args(paths).exec()?;
    Ok(())
}

/// Stage pathspecs into the index (like `git add`),
///
/// Treats each item in `paths` as a pathspec and passes the collection to
/// [`git2::Index::add_all`]. Ignores honored; re‑adding existing staged entries is a no‑op.
///
/// Supported pathspecs:
/// - Files: "src/main.rs"
/// - Directories (recursive): "src/"
/// - Globs (libgit2 syntax): "*.rs", "docs/**/*.md"
/// - Mixed file + pattern list
///
/// # Arguments
/// - `repo` Open repository whose index will be modified.
/// - `paths` Iterator of pathspecs. Empty iterator = no‑op.
///
/// # Returns
/// [`Result::Ok`] (()) on success.
///
/// # Errors
/// - Loading index fails.
/// - Applying any pathspec fails.
/// - Writing updated index fails.
///
/// # Future Work
/// - Expose force option to include otherwise ignored files.
/// - Return count of affected entries for diagnostics.
pub fn add_to_index<T, I>(repo: &mut Repository, paths: I) -> color_eyre::Result<()>
where
    T: IntoCString,
    I: IntoIterator<Item = T>,
{
    let mut index = repo.index()?;
    index.add_all(paths, git2::IndexAddOption::DEFAULT, None)?;
    index.write()?;
    Ok(())
}

/// Fetches all branches from the 'origin' remote and returns all local and remote [`Branch`]es
/// sorted by last committer date (most recent first).
///
/// # Errors
/// - The repository cannot be discovered.
/// - The 'origin' remote cannot be found.
/// - Performing `git fetch` for all branches fails.
/// - Enumerating branches fails.
/// - A branch name is not valid UTF-8.
/// - Resolving the branch tip commit fails.
/// - Converting the committer timestamp into a [`DateTime`] fails.
pub fn get_branches() -> color_eyre::Result<Vec<Branch>> {
    let repo = get_repo(Path::new("."))?;
    fetch_branches(&[])?;

    let mut out = vec![];
    for branch_res in repo.branches(None)? {
        out.push(Branch::try_from(branch_res?)?);
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

/// Fetches the specified branch names from the `origin` remote.
///
/// Used before switching to a branch that may only exist remotely
/// (e.g. derived from a GitHub PR URL).
///
/// # Errors
/// - The repository cannot be discovered.
/// - The `origin` remote cannot be found.
/// - Performing `git fetch` for the requested branches fails.
pub fn fetch_branches(branches: &[&str]) -> color_eyre::Result<()> {
    let repo = get_repo(Path::new("."))?;

    let mut callbacks = RemoteCallbacks::new();
    callbacks.credentials(|_url, username_from_url, _allowed_types| {
        Cred::ssh_key_from_agent(username_from_url.unwrap_or("git"))
    });

    let mut fetch_opts = git2::FetchOptions::new();
    fetch_opts.remote_callbacks(callbacks);

    repo.find_remote("origin")?
        .fetch(branches, Some(&mut fetch_opts), None)?;

    Ok(())
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

/// Attempts to convert a libgit2 branch and its type into our [`Branch`] enum.
///
/// Extracts the branch name and last committer date from the raw branch.
///
/// # Errors
/// - Branch name is not valid UTF-8.
/// - Resolving the branch tip commit fails.
/// - Converting the committer timestamp into a [`DateTime`] fails.
impl<'a> TryFrom<(git2::Branch<'a>, git2::BranchType)> for Branch {
    type Error = color_eyre::eyre::Error;

    fn try_from((raw_branch, branch_type): (git2::Branch<'a>, git2::BranchType)) -> Result<Self, Self::Error> {
        let branch_name = raw_branch
            .name()?
            .ok_or_else(|| eyre!("branch name invalid utf-8 | input=raw_branch.name()"))?;
        let commit_time = raw_branch.get().peel_to_commit()?.committer().when();
        let committer_date_time = DateTime::from_timestamp(commit_time.seconds(), 0)
            .ok_or_else(|| eyre!("invalid commit timestamp | seconds={}", commit_time.seconds()))?;

        Ok(match branch_type {
            git2::BranchType::Local => Self::Local {
                name: branch_name.to_string(),
                committer_date_time,
            },
            git2::BranchType::Remote => Self::Remote {
                name: branch_name.to_string(),
                committer_date_time,
            },
        })
    }
}

/// Combined staged + worktree status for a path
///
/// Aggregates index + worktree bitflags plus conflict / ignore markers into a higher‑level
/// representation with convenience predicates (e.g. [`GitStatusEntry::is_new`]).
#[derive(Clone, Debug)]
#[cfg_attr(test, derive(PartialEq, Eq))]
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
        if self.is_new_in_index() || self.worktree_state.as_ref().is_some_and(WorktreeState::is_new) {
            return true;
        }
        false
    }

    pub fn is_new_in_index(&self) -> bool {
        self.index_state.as_ref().is_some_and(IndexState::is_new)
    }
}

impl TryFrom<(PathBuf, &StatusEntry<'_>)> for GitStatusEntry {
    type Error = color_eyre::eyre::Error;

    fn try_from((repo_root, value): (PathBuf, &StatusEntry<'_>)) -> Result<Self, Self::Error> {
        let status = value.status();
        let path = value
            .path()
            .map(PathBuf::from)
            .ok_or_else(|| eyre!("missing status path | context=StatusEntry"))?;

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
#[derive(Clone, Debug)]
#[cfg_attr(test, derive(PartialEq, Eq))]
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
#[derive(Clone, Debug)]
#[cfg_attr(test, derive(PartialEq, Eq))]
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

#[cfg(test)]
mod tests {
    use git2::Repository;
    use git2::Signature;
    use git2::Time;
    use rstest::rstest;
    use tempfile::TempDir;

    use super::*;

    #[rstest]
    #[case::remote_same_short_name(
        vec![local("feature-x"), remote("origin/feature-x")],
        vec![local("feature-x")]
    )]
    #[case::no_redundant(
        vec![local("feature-x"), remote("origin/feature-y")],
        vec![local("feature-x"), remote("origin/feature-y")]
    )]
    #[case::multiple_mixed(
        vec![
            local("feature-x"),
            remote("origin/feature-x"),
            remote("origin/feature-y"),
            local("main"),
            remote("upstream/main")
        ],
        vec![local("feature-x"), remote("origin/feature-y"), local("main")]
    )]
    #[case::different_remote_prefix(
        vec![local("feature-x"), remote("upstream/feature-x")],
        vec![local("feature-x")]
    )]
    fn remove_redundant_remotes_cases(#[case] mut input: Vec<Branch>, #[case] expected: Vec<Branch>) {
        remove_redundant_remotes(&mut input);
        assert_eq!(input, expected);
    }

    #[rstest]
    #[case::index_new(Some(IndexState::New), None, true)]
    #[case::worktree_new(None, Some(WorktreeState::New), true)]
    #[case::both_new(Some(IndexState::New), Some(WorktreeState::New), true)]
    #[case::modified_index(Some(IndexState::Modified), None, false)]
    #[case::modified_worktree(None, Some(WorktreeState::Modified), false)]
    #[case::none(None, None, false)]
    fn git_status_entry_is_new_cases(
        #[case] index_state: Option<IndexState>,
        #[case] worktree_state: Option<WorktreeState>,
        #[case] expected: bool,
    ) {
        let entry = entry(index_state, worktree_state);
        assert_eq!(entry.is_new(), expected);
    }

    #[rstest]
    #[case(Status::INDEX_NEW, Some(IndexState::New))]
    #[case(Status::INDEX_MODIFIED, Some(IndexState::Modified))]
    #[case(Status::INDEX_DELETED, Some(IndexState::Deleted))]
    #[case(Status::INDEX_RENAMED, Some(IndexState::Renamed))]
    #[case(Status::INDEX_TYPECHANGE, Some(IndexState::Typechange))]
    #[case(Status::WT_MODIFIED, None)]
    fn index_state_new_maps_each_flag(#[case] input: Status, #[case] expected: Option<IndexState>) {
        assert_eq!(IndexState::new(&input), expected);
    }

    #[rstest]
    #[case(Status::WT_NEW, Some(WorktreeState::New))]
    #[case(Status::WT_MODIFIED, Some(WorktreeState::Modified))]
    #[case(Status::WT_DELETED, Some(WorktreeState::Deleted))]
    #[case(Status::WT_RENAMED, Some(WorktreeState::Renamed))]
    #[case(Status::WT_TYPECHANGE, Some(WorktreeState::Typechange))]
    #[case(Status::WT_UNREADABLE, Some(WorktreeState::Unreadable))]
    #[case(Status::INDEX_MODIFIED, None)]
    fn worktree_state_new_maps_each_flag(#[case] input: Status, #[case] expected: Option<WorktreeState>) {
        assert_eq!(WorktreeState::new(&input), expected);
    }

    #[test]
    fn branch_try_from_converts_local_branch_successfully() {
        let (_temp_dir, repo) = init_test_repo(Some(Time::new(42, 3)));

        let head_commit = repo.head().unwrap().peel_to_commit().unwrap();
        let branch = repo.branch("test-branch", &head_commit, false).unwrap();

        assert2::let_assert!(Ok(result) = Branch::try_from((branch, git2::BranchType::Local)));

        pretty_assertions::assert_eq!(
            result,
            Branch::Local {
                name: "test-branch".to_string(),
                committer_date_time: DateTime::from_timestamp(42, 0).unwrap(),
            }
        );
    }

    fn local(name: &str) -> Branch {
        Branch::Local {
            name: name.into(),
            committer_date_time: DateTime::from_timestamp(0, 0).unwrap(),
        }
    }

    fn remote(name: &str) -> Branch {
        Branch::Remote {
            name: name.into(),
            committer_date_time: DateTime::from_timestamp(0, 0).unwrap(),
        }
    }

    fn entry(index_state: Option<IndexState>, worktree_state: Option<WorktreeState>) -> GitStatusEntry {
        GitStatusEntry {
            path: "p".into(),
            repo_root: ".".into(),
            conflicted: false,
            ignored: false,
            index_state,
            worktree_state,
        }
    }

    fn init_test_repo(time: Option<Time>) -> (TempDir, Repository) {
        let temp_dir = TempDir::new().unwrap();
        let repo = Repository::init(temp_dir.path()).unwrap();

        // Dummy initial commit
        let mut index = repo.index().unwrap();
        let oid = index.write_tree().unwrap();
        let tree = repo.find_tree(oid).unwrap();
        let sig = time.map_or_else(
            || Signature::now("test", "test@example.com").unwrap(),
            |time| Signature::new("test", "test@example.com", &time).unwrap(),
        );
        repo.commit(Some("HEAD"), &sig, &sig, "initial", &tree, &[]).unwrap();

        drop(tree);

        (temp_dir, repo)
    }
}
