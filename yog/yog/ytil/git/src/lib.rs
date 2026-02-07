//! Lightweight Git helpers atop [`git2`] with fallbacks to `git` CLI.

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;

use git2::IntoCString;
pub use git2::Repository;
use git2::Status;
use git2::StatusEntry;
use git2::StatusOptions;
use rootcause::prelude::ResultExt;
use rootcause::report;
pub use ytil_cmd::CmdError;
use ytil_cmd::CmdExt as _;

pub mod branch;
pub mod diff;
pub mod remote;
pub mod repo;

/// Enumerate combined staged + unstaged status entries.
///
/// # Errors
/// - Repository discovery, status reading, or entry construction fails.
pub fn get_status() -> rootcause::Result<Vec<GitStatusEntry>> {
    let repo = crate::repo::discover(Path::new("."))
        .context("error getting repo")
        .attach("operation=status")?;
    let repo_root = Arc::new(crate::repo::get_root(&repo));

    let mut opts = StatusOptions::default();
    opts.include_untracked(true);
    opts.include_ignored(false);

    let mut out = vec![];
    for status_entry in repo
        .statuses(Some(&mut opts))
        .context("error getting statuses")
        .attach_with(|| format!("repo_root={}", repo_root.display()))?
        .iter()
    {
        out.push(
            GitStatusEntry::try_from((Arc::clone(&repo_root), &status_entry))
                .context("error creating status entry")
                .attach_with(|| format!("repo_root={}", repo_root.display()))?,
        );
    }
    Ok(out)
}

/// Restore one or more paths from index or optional branch.
///
/// # Errors
/// - `git restore` command fails.
pub fn restore<I, P>(paths: I, branch: Option<&str>) -> rootcause::Result<()>
where
    I: IntoIterator<Item = P>,
    P: AsRef<str>,
{
    let mut cmd = Command::new("git");
    cmd.arg("restore");
    if let Some(branch) = branch {
        cmd.arg(format!("--source={branch}"));
    }
    for p in paths {
        cmd.arg(p.as_ref());
    }
    cmd.exec()?;
    Ok(())
}

/// Unstage specific paths without touching working tree contents.
///
/// # Errors
/// - `git restore --staged` command fails.
pub fn unstage<I, P>(paths: I) -> rootcause::Result<()>
where
    I: IntoIterator<Item = P>,
    P: AsRef<str>,
{
    // Use porcelain `git restore --staged` which modifies only the index (opposite of `git add`).
    // This avoids resurrecting deleted files (observed when using libgit2 `reset_default`).
    let mut cmd = Command::new("git");
    cmd.args(["restore", "--staged"]);
    let mut has_paths = false;
    for p in paths {
        cmd.arg(p.as_ref());
        has_paths = true;
    }
    if !has_paths {
        return Ok(());
    }
    cmd.exec().context("error restoring staged Git entries")?;
    Ok(())
}

/// Stage pathspecs into the index (like `git add`).
///
/// # Errors
/// - Loading, updating, or writing index fails.
pub fn add_to_index<T, I>(repo: &mut Repository, paths: I) -> rootcause::Result<()>
where
    T: IntoCString,
    I: IntoIterator<Item = T>,
{
    let mut index = repo.index().context("error loading index")?;
    index
        .add_all(paths, git2::IndexAddOption::DEFAULT, None)
        .context("error adding paths to index")?;
    index.write().context("error writing index")?;
    Ok(())
}

/// Retrieves the commit hash of the current HEAD.
///
/// # Errors
/// - HEAD resolution fails.
pub fn get_current_commit_hash(repo: &Repository) -> rootcause::Result<String> {
    let head = repo.head().context("error getting repo head")?;
    let commit = head.peel_to_commit().context("error peeling head to commit")?;
    Ok(commit.id().to_string())
}

/// Combined staged + worktree status for a path.
#[derive(Clone, Debug)]
#[cfg_attr(test, derive(Eq, PartialEq))]
pub struct GitStatusEntry {
    pub path: PathBuf,
    /// Shared repository root; uses `Arc` to avoid cloning the `PathBuf` per entry.
    pub repo_root: Arc<PathBuf>,
    pub conflicted: bool,
    pub ignored: bool,
    pub index_state: Option<IndexState>,
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

    /// Returns `true` if the entry has any staged (index) changes.
    pub const fn is_staged(&self) -> bool {
        self.index_state.is_some()
    }
}

impl TryFrom<(Arc<PathBuf>, &StatusEntry<'_>)> for GitStatusEntry {
    type Error = rootcause::Report;

    fn try_from((repo_root, value): (Arc<PathBuf>, &StatusEntry<'_>)) -> Result<Self, Self::Error> {
        let status = value.status();
        let path = value
            .path()
            .map(PathBuf::from)
            .ok_or_else(|| report!("error missing status path"))
            .attach_with(|| "context=StatusEntry".to_string())?;

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
    #[case::index_new(Some(IndexState::New), None, true)]
    #[case::index_modified(Some(IndexState::Modified), None, true)]
    #[case::index_deleted(Some(IndexState::Deleted), None, true)]
    #[case::index_renamed(Some(IndexState::Renamed), None, true)]
    #[case::index_typechange(Some(IndexState::Typechange), None, true)]
    #[case::worktree_only_modified(None, Some(WorktreeState::Modified), false)]
    #[case::worktree_only_new(None, Some(WorktreeState::New), false)]
    #[case::both_staged_and_worktree(Some(IndexState::Modified), Some(WorktreeState::Modified), true)]
    #[case::none(None, None, false)]
    fn git_status_entry_is_staged_cases(
        #[case] index_state: Option<IndexState>,
        #[case] worktree_state: Option<WorktreeState>,
        #[case] expected: bool,
    ) {
        let entry = entry(index_state, worktree_state);
        assert_eq!(entry.is_staged(), expected);
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
            repo_root: Arc::new(".".into()),
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
