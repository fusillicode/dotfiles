//! Provide lightweight Git helpers atop [`git2`] plus selective fallbacks to the system `git` binary.
//!
//! Wrap common operations (repo discovery, root resolution, status enumeration, branch listing,
//! targeted fetch, branch switching, restore) in focused functions returning structured data
//! ([`GitStatusEntry`], [`branch::Branch`]). Some semantics (previous branch with `switch -`, restore) defer to
//! the porcelain CLI to avoid re‑implementing complex behavior.

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use color_eyre::eyre::WrapErr;
use color_eyre::eyre::eyre;
use git2::IntoCString;
pub use git2::Repository;
use git2::Status;
use git2::StatusEntry;
use git2::StatusOptions;
pub use ytil_cmd::CmdError;
use ytil_cmd::CmdExt as _;

pub mod branch;
pub mod diff;
pub mod remote;

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
pub fn discover_repo(path: &Path) -> color_eyre::Result<Repository> {
    Repository::discover(path).wrap_err_with(|| eyre!("error discovering repo | path={}", path.display()))
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
    let repo = discover_repo(Path::new(".")).wrap_err_with(|| eyre!("error getting repo | operation=status"))?;
    let repo_root = get_repo_root(&repo);

    let mut opts = StatusOptions::default();
    opts.include_untracked(true);
    opts.include_ignored(false);

    let mut out = vec![];
    for status_entry in repo
        .statuses(Some(&mut opts))
        .wrap_err_with(|| eyre!("error getting statuses | repo_root={}", repo_root.display()))?
        .iter()
    {
        out.push(
            GitStatusEntry::try_from((repo_root.clone(), &status_entry))
                .wrap_err_with(|| eyre!("error creating status entry | repo_root={}", repo_root.display()))?,
        );
    }
    Ok(out)
}

/// Restore one or more paths from index or optional branch,
///
/// Delegates to porcelain `git restore` rather than approximating behavior with libgit2.
/// If `branch` is provided its tree is the source; otherwise the index / HEAD is used.
///
/// # Arguments
/// - `paths` Iterator of absolute or relative paths to restore. Empty iterator = no‑op.
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
pub fn restore<I, P>(paths: I, branch: Option<&str>) -> color_eyre::Result<()>
where
    I: IntoIterator<Item = P>,
    P: AsRef<str>,
{
    let mut cmd = Command::new("git");
    cmd.arg("restore");
    if let Some(branch) = branch {
        cmd.arg(branch);
    }
    for p in paths {
        cmd.arg(p.as_ref());
    }
    cmd.exec()?;
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
    Command::new("git")
        .args(["restore", "--staged"])
        .args(paths)
        .exec()
        .wrap_err_with(|| eyre!("error restoring statged Git entries | paths={paths:?}"))?;
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
    let mut index = repo.index().wrap_err_with(|| eyre!("error loading index"))?;
    index
        .add_all(paths, git2::IndexAddOption::DEFAULT, None)
        .wrap_err_with(|| eyre!("error adding paths to index"))?;
    index.write().wrap_err_with(|| eyre!("error writing index"))?;
    Ok(())
}

/// Retrieves the commit hash of the current HEAD.
///
/// # Returns
/// The commit hash as a hexadecimal string.
///
/// # Errors
/// - If the repository cannot be opened.
/// - If the HEAD reference cannot be resolved.
/// - If the HEAD reference does not point to a commit.
pub fn get_current_commit_hash(repo: &Repository) -> color_eyre::Result<String> {
    let head = repo.head().wrap_err_with(|| eyre!("error getting repo head"))?;
    let commit = head
        .peel_to_commit()
        .wrap_err_with(|| eyre!("error peeling head to commit"))?;
    Ok(commit.id().to_string())
}

/// Combined staged + worktree status for a path
///
/// Aggregates index + worktree bitflags plus conflict / ignore markers into a higher‑level
/// representation with convenience predicates (e.g. [`GitStatusEntry::is_new`]).
#[derive(Clone, Debug)]
#[cfg_attr(test, derive(Eq, PartialEq))]
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
            .ok_or_else(|| eyre!("error missing status path | context=StatusEntry"))?;

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
#[cfg_attr(test, derive(Eq, PartialEq))]
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
#[cfg_attr(test, derive(Eq, PartialEq))]
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

    pub fn init_test_repo(time: Option<Time>) -> (TempDir, Repository) {
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
