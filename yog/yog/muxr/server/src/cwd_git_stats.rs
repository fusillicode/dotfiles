use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::MutexGuard;

use git2::Repository;
use git2::Status;
use git2::StatusOptions;
use muxr_core::GitStats;
use muxr_core::PaneId;
use rootcause::report;
use tokio::sync::mpsc;

use crate::cmd_label::TerminalTitle;
use crate::state::PaneSnapshotFields;
use crate::state::SessionLayout;

const GIT_STATS_WAKE_CHANNEL_LIMIT: usize = 1;
const GIT_STATS_RESULT_CHANNEL_LIMIT: usize = 32;

type PendingCwdGitStatsRefreshes = Arc<Mutex<BTreeMap<String, CwdGitStatsRefresh>>>;

#[derive(Clone, Debug, Eq, PartialEq)]
struct RepoGitStatsSnapshot {
    repo_workdir: String,
    stats: GitStats,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CwdGitStatsRefresh {
    cwd: String,
    generation: u64,
}

impl CwdGitStatsRefresh {
    fn into_result(self, snapshot: Option<RepoGitStatsSnapshot>) -> CwdGitStatsResult {
        CwdGitStatsResult {
            cwd: self.cwd,
            generation: self.generation,
            snapshot,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CwdGitStatsResult {
    cwd: String,
    generation: u64,
    snapshot: Option<RepoGitStatsSnapshot>,
}

struct RepoGitStatsRefreshes {
    repo: Repository,
    refreshes: Vec<CwdGitStatsRefresh>,
}

#[derive(Clone)]
pub struct CwdGitStatsRequester {
    pending: PendingCwdGitStatsRefreshes,
    wake_sender: mpsc::Sender<()>,
}

impl CwdGitStatsRequester {
    pub fn request_refreshes(&self, refreshes: Vec<CwdGitStatsRefresh>) -> rootcause::Result<()> {
        if refreshes.is_empty() {
            return Ok(());
        }

        {
            let mut pending = self::lock_pending_refreshes(&self.pending)?;
            for refresh in refreshes {
                pending.insert(refresh.cwd.clone(), refresh);
            }
        }

        // Wakeups are deliberately one-slot: a full channel means the worker already has, or is about to receive, a
        // wakeup. Refresh payloads stay in the cwd-keyed pending map, so bursts replace stale per-cwd work instead of
        // spawning unbounded parked send tasks.
        let _wake_sent = self.wake_sender.try_send(());
        Ok(())
    }
}

pub fn cwd_git_stats_worker() -> (CwdGitStatsRequester, mpsc::Receiver<Vec<CwdGitStatsResult>>) {
    let pending = Arc::new(Mutex::new(BTreeMap::new()));
    let (wake_sender, wake_receiver) = mpsc::channel(GIT_STATS_WAKE_CHANNEL_LIMIT);
    let (result_sender, result_receiver) = mpsc::channel(GIT_STATS_RESULT_CHANNEL_LIMIT);
    tokio::spawn(self::run_cwd_git_stats_worker(
        Arc::clone(&pending),
        wake_receiver,
        result_sender,
    ));
    (CwdGitStatsRequester { pending, wake_sender }, result_receiver)
}

#[derive(Debug, Default)]
pub struct CwdGitStats {
    latest_generation_by_cwd: BTreeMap<String, u64>,
    next_generation: u64,
    pending_submit_cwd_by_pane: BTreeMap<PaneId, String>,
    // Git stats belong to the repo workdir, not to the triggering pane: split panes can show different cwd strings
    // inside the same repo, and the sidebar display pane can change without a new git scan.
    stats_by_repo_workdir: BTreeMap<String, GitStats>,
}

impl CwdGitStats {
    pub fn mark_shell_submit_cwd(&mut self, pane_id: PaneId, cwd: String) {
        self.pending_submit_cwd_by_pane.insert(pane_id, cwd);
    }

    pub fn prepare_refreshes(
        &mut self,
        layout: &SessionLayout,
        snapshot_fields: &PaneSnapshotFields,
        pane_ids: impl IntoIterator<Item = PaneId>,
    ) -> Vec<CwdGitStatsRefresh> {
        let pane_ids = pane_ids.into_iter().collect::<BTreeSet<_>>();
        let cwd_by_pane = layout
            .pane_cwds_with_runtime_metadata(snapshot_fields)
            .into_iter()
            .collect::<BTreeMap<_, _>>();
        let mut refreshes = Vec::new();
        for pane_id in pane_ids {
            let Some(cwd) = cwd_by_pane.get(&pane_id) else {
                continue;
            };
            refreshes.push(self.next_refresh(cwd.clone()));
        }
        self.retain_layout_panes(layout);
        refreshes
    }

    pub fn clear_pending_submit_cwds_for_panes(&mut self, pane_ids: impl IntoIterator<Item = PaneId>) {
        let pane_ids = pane_ids.into_iter().collect::<BTreeSet<_>>();
        self.pending_submit_cwd_by_pane
            .retain(|pane_id, _cwd| !pane_ids.contains(pane_id));
    }

    pub fn take_ready_submit_refreshes(
        &mut self,
        layout: &SessionLayout,
        terminal_titles: &[(PaneId, Option<String>)],
    ) -> Vec<CwdGitStatsRefresh> {
        let mut refreshes = Vec::new();
        for (pane_id, terminal_title) in terminal_titles {
            if !self.pending_submit_cwd_by_pane.contains_key(pane_id) {
                continue;
            }
            let Some(pane) = layout.pane(*pane_id) else {
                continue;
            };
            if TerminalTitle::classify(terminal_title.as_deref(), &pane.cwd)
                .cwd
                .is_some()
                && let Some(cwd) = self.pending_submit_cwd_by_pane.remove(pane_id)
            {
                refreshes.push(self.next_refresh(cwd));
            }
        }
        self.retain_layout_panes(layout);
        refreshes
    }

    pub fn apply_results(
        &mut self,
        layout: &SessionLayout,
        snapshot_fields: &PaneSnapshotFields,
        results: Vec<CwdGitStatsResult>,
    ) -> Vec<PaneId> {
        let pane_ids = layout.panes().into_iter().map(|pane| pane.id).collect::<BTreeSet<_>>();
        let cwd_by_pane = layout
            .pane_cwds_with_runtime_metadata(snapshot_fields)
            .into_iter()
            .collect::<BTreeMap<_, _>>();
        let previous_stats_by_pane = self::git_stats_by_pane(self, &cwd_by_pane);
        for result in results {
            if self.latest_generation_by_cwd.get(&result.cwd) != Some(&result.generation) {
                continue;
            }
            // Accepted results are single-use. Keeping completed cwd generations would grow session state while stale
            // result protection only needs cwds that still have pending work.
            self.latest_generation_by_cwd.remove(&result.cwd);
            let Some(snapshot) = result.snapshot else {
                self.clear_cached_stats_for_cwd(&result.cwd);
                continue;
            };
            self.stats_by_repo_workdir.insert(snapshot.repo_workdir, snapshot.stats);
        }
        self.retain_layout_panes(layout);
        let current_stats_by_pane = self::git_stats_by_pane(self, &cwd_by_pane);
        pane_ids
            .into_iter()
            .filter(|pane_id| previous_stats_by_pane.get(pane_id) != current_stats_by_pane.get(pane_id))
            .collect()
    }

    pub fn populate_snapshot_fields(&self, layout: &SessionLayout, snapshot_fields: &mut PaneSnapshotFields) {
        let cwd_by_pane = layout.pane_cwds_with_runtime_metadata(snapshot_fields);
        for (pane_id, cwd) in cwd_by_pane {
            snapshot_fields.set_git_stats(pane_id, self.git_stats_for_cwd(&cwd));
        }
    }

    const fn next_generation(&mut self) -> u64 {
        self.next_generation = self.next_generation.saturating_add(1);
        self.next_generation
    }

    fn retain_layout_panes(&mut self, layout: &SessionLayout) {
        let pane_ids = layout.panes().into_iter().map(|pane| pane.id).collect::<BTreeSet<_>>();
        self.pending_submit_cwd_by_pane
            .retain(|pane_id, _cwd| pane_ids.contains(pane_id));
    }

    fn next_refresh(&mut self, cwd: String) -> CwdGitStatsRefresh {
        let generation = self.next_generation();
        self.latest_generation_by_cwd.insert(cwd.clone(), generation);
        CwdGitStatsRefresh { cwd, generation }
    }

    fn clear_cached_stats_for_cwd(&mut self, cwd: &str) {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let repo_workdirs = self
            .stats_by_repo_workdir
            .keys()
            .filter(|repo_workdir| self::cwd_is_under_repo_workdir(cwd, repo_workdir, home.as_deref()))
            .cloned()
            .collect::<Vec<_>>();
        for repo_workdir in repo_workdirs {
            self.stats_by_repo_workdir.remove(&repo_workdir);
        }
    }

    fn git_stats_for_cwd(&self, cwd: &str) -> Option<GitStats> {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        self.stats_by_repo_workdir
            .iter()
            .filter(|(repo_workdir, _stats)| self::cwd_is_under_repo_workdir(cwd, repo_workdir, home.as_deref()))
            .max_by_key(|(repo_workdir, _stats)| repo_workdir.len())
            .map(|(_repo_workdir, stats)| *stats)
    }
}

async fn run_cwd_git_stats_worker(
    pending: PendingCwdGitStatsRefreshes,
    mut wake_receiver: mpsc::Receiver<()>,
    result_sender: mpsc::Sender<Vec<CwdGitStatsResult>>,
) {
    // Git stats can walk large repos. Keep it off the attached-client loop, coalesce queued work by cwd, group scans
    // by repo workdir inside each blocking batch, and let cwd generations discard stale results.
    while wake_receiver.recv().await.is_some() {
        let Ok(refreshes) = self::take_pending_refreshes(&pending) else {
            break;
        };
        if refreshes.is_empty() {
            continue;
        }
        let Ok(results) = tokio::task::spawn_blocking(move || self::git_stats_results_for_refreshes(refreshes)).await
        else {
            continue;
        };
        if result_sender.send(results).await.is_err() {
            break;
        }
    }
}

fn take_pending_refreshes(pending: &PendingCwdGitStatsRefreshes) -> rootcause::Result<Vec<CwdGitStatsRefresh>> {
    Ok(std::mem::take(&mut *self::lock_pending_refreshes(pending)?)
        .into_values()
        .collect())
}

fn lock_pending_refreshes(
    pending: &PendingCwdGitStatsRefreshes,
) -> rootcause::Result<MutexGuard<'_, BTreeMap<String, CwdGitStatsRefresh>>> {
    pending
        .lock()
        .map_err(|_| report!("poisoned muxr git stats pending refreshes mutex"))
}

fn git_stats_results_for_refreshes(refreshes: Vec<CwdGitStatsRefresh>) -> Vec<CwdGitStatsResult> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    self::git_stats_results_for_refreshes_with_home(refreshes, home.as_deref())
}

fn git_stats_results_for_refreshes_with_home(
    refreshes: Vec<CwdGitStatsRefresh>,
    home: Option<&Path>,
) -> Vec<CwdGitStatsResult> {
    let (mut results, refreshes_by_repo_workdir) = self::group_refreshes_by_repo_workdir(refreshes, home);
    for (repo_workdir, repo_refreshes) in refreshes_by_repo_workdir {
        let snapshot = Some(self::repo_git_stats_snapshot(repo_workdir, &repo_refreshes.repo));
        results.extend(
            repo_refreshes
                .refreshes
                .into_iter()
                .map(|refresh| refresh.into_result(snapshot.clone())),
        );
    }
    results
}

fn group_refreshes_by_repo_workdir(
    refreshes: Vec<CwdGitStatsRefresh>,
    home: Option<&Path>,
) -> (Vec<CwdGitStatsResult>, BTreeMap<String, RepoGitStatsRefreshes>) {
    let mut non_git_results = Vec::new();
    let mut refreshes_by_repo_workdir: BTreeMap<String, RepoGitStatsRefreshes> = BTreeMap::new();
    for refresh in refreshes {
        let Some((repo_workdir, repo)) = self::discover_repo_for_path(Path::new(&refresh.cwd), home) else {
            non_git_results.push(refresh.into_result(None));
            continue;
        };
        if let Some(repo_refreshes) = refreshes_by_repo_workdir.get_mut(&repo_workdir) {
            repo_refreshes.refreshes.push(refresh);
        } else {
            refreshes_by_repo_workdir.insert(
                repo_workdir,
                RepoGitStatsRefreshes {
                    repo,
                    refreshes: vec![refresh],
                },
            );
        }
    }
    (non_git_results, refreshes_by_repo_workdir)
}

fn discover_repo_for_path(path: &Path, home: Option<&Path>) -> Option<(String, Repository)> {
    let discovery_path = self::expand_home(path, home);
    let repo = Repository::discover(discovery_path).ok()?;
    let repo_workdir = repo.workdir().map(self::path_key)?;
    Some((repo_workdir, repo))
}

fn repo_git_stats_snapshot(repo_workdir: String, repo: &Repository) -> RepoGitStatsSnapshot {
    RepoGitStatsSnapshot {
        repo_workdir,
        stats: self::stats_for_repo(repo),
    }
}

fn stats_for_repo(repo: &Repository) -> GitStats {
    let (insertions, deletions) = repo
        .diff_index_to_workdir(None, None)
        .and_then(|diff| diff.stats())
        .map_or((0, 0), |stats| (stats.insertions(), stats.deletions()));
    let mut status_options = self::status_options();
    let new_files = repo.statuses(Some(&mut status_options)).map_or(0, |statuses| {
        statuses
            .iter()
            .filter(|entry| entry.status().contains(Status::WT_NEW))
            .count()
    });
    GitStats {
        deletions: self::saturating_u32(deletions),
        insertions: self::saturating_u32(insertions),
        new_files: self::saturating_u32(new_files),
    }
}

fn expand_home(path: &Path, home: Option<&Path>) -> PathBuf {
    let Some(raw_path) = path.to_str() else {
        return path.to_path_buf();
    };
    let Some(home) = home else {
        return path.to_path_buf();
    };
    if raw_path == "~" {
        return home.to_path_buf();
    }
    raw_path
        .strip_prefix("~/")
        .map_or_else(|| path.to_path_buf(), |rest| home.join(rest))
}

fn path_key(path: &Path) -> String {
    let key = self::normalize_existing_path(path).to_string_lossy().to_string();
    if key.len() > 1 {
        key.trim_end_matches('/').to_owned()
    } else {
        key
    }
}

fn normalize_existing_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn cwd_is_under_repo_workdir(cwd: &str, repo_workdir: &str, home: Option<&Path>) -> bool {
    self::normalize_existing_path(&self::expand_home(Path::new(cwd), home)).starts_with(Path::new(repo_workdir))
}

fn git_stats_by_pane(
    cwd_git_stats: &CwdGitStats,
    cwd_by_pane: &BTreeMap<PaneId, String>,
) -> BTreeMap<PaneId, Option<GitStats>> {
    cwd_by_pane
        .iter()
        .map(|(pane_id, cwd)| (*pane_id, cwd_git_stats.git_stats_for_cwd(cwd)))
        .collect()
}

fn status_options() -> StatusOptions {
    let mut options = StatusOptions::new();
    options
        .include_untracked(true)
        .recurse_untracked_dirs(true)
        .exclude_submodules(true)
        .include_ignored(false);
    options
}

fn saturating_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::Arc;
    use std::sync::Mutex;

    use git2::Signature;
    use muxr_core::SessionName;
    use rootcause::prelude::ResultExt;
    use tempfile::TempDir;

    use super::*;
    use crate::pane_split::PaneSplitAxis;
    use crate::pane_split::PaneSplitRatio;
    use crate::state::PaneTree;
    use crate::state::SessionMetadata;

    #[test]
    fn test_repo_git_stats_for_path_when_path_is_not_git_repo_returns_none() -> rootcause::Result<()> {
        let temp = TempDir::new().context("failed to create temp dir")?;

        pretty_assertions::assert_eq!(
            repo_git_stats_snapshot_for_path(temp.path()).map(|snapshot| snapshot.stats),
            None
        );
        Ok(())
    }

    #[test]
    fn test_repo_git_stats_for_path_when_repo_is_clean_returns_clean_stats() -> rootcause::Result<()> {
        let temp = self::repo_with_initial_file()?;

        pretty_assertions::assert_eq!(
            repo_git_stats_snapshot_for_path(temp.path()).map(|snapshot| snapshot.stats),
            Some(GitStats::default()),
        );
        Ok(())
    }

    #[test]
    fn test_repo_git_stats_for_path_when_worktree_is_dirty_returns_agg_style_counts() -> rootcause::Result<()> {
        let temp = self::repo_with_initial_file()?;
        fs::write(temp.path().join("tracked.txt"), "new\nextra\n").context("failed to modify tracked file")?;
        fs::write(temp.path().join("untracked.txt"), "untracked\n").context("failed to write untracked file")?;

        let stats = repo_git_stats_snapshot_for_path(temp.path())
            .map(|snapshot| snapshot.stats)
            .ok_or_else(|| rootcause::report!("expected git stats for temp repo"))?;

        assert2::assert!(stats.insertions > 0);
        assert2::assert!(stats.deletions > 0);
        pretty_assertions::assert_eq!(stats.new_files, 1);
        Ok(())
    }

    #[test]
    fn test_repo_git_stats_for_path_when_path_uses_home_prefix_expands_for_discovery() -> rootcause::Result<()> {
        let home = TempDir::new().context("failed to create temp home dir")?;
        let repo_path = home.path().join("repo");
        fs::create_dir(&repo_path).context("failed to create temp home repo dir")?;
        self::init_repo_with_initial_file(&repo_path)?;

        pretty_assertions::assert_eq!(
            repo_git_stats_snapshot_for_path_with_home(Path::new("~/repo"), Some(home.path()))
                .map(|snapshot| snapshot.stats),
            Some(GitStats::default()),
        );
        Ok(())
    }

    #[test]
    fn test_cwd_git_stats_when_repo_changes_keeps_cached_stats_until_pane_refresh() -> rootcause::Result<()> {
        let temp = self::repo_with_initial_file()?;
        let mut layout = self::layout_for_cwd(temp.path().to_string_lossy())?;
        let pane_id = PaneId::new(1)?;
        let mut cwd_git_stats = CwdGitStats::default();
        let snapshot_fields = PaneSnapshotFields::default();

        let results = self::git_stats_results_for_refreshes(cwd_git_stats.prepare_refreshes(
            &layout,
            &snapshot_fields,
            [pane_id],
        ));
        pretty_assertions::assert_eq!(
            cwd_git_stats.apply_results(&layout, &snapshot_fields, results),
            vec![pane_id]
        );
        let mut fields = PaneSnapshotFields::default();
        cwd_git_stats.populate_snapshot_fields(&layout, &mut fields);
        pretty_assertions::assert_eq!(fields.git_stats(pane_id), Some(GitStats::default()));

        fs::write(temp.path().join("untracked.txt"), "untracked\n").context("failed to write untracked file")?;
        let mut cached_fields = PaneSnapshotFields::default();
        cwd_git_stats.populate_snapshot_fields(&layout, &mut cached_fields);
        pretty_assertions::assert_eq!(cached_fields.git_stats(pane_id), Some(GitStats::default()));

        let results = self::git_stats_results_for_refreshes(cwd_git_stats.prepare_refreshes(
            &layout,
            &snapshot_fields,
            [pane_id],
        ));
        pretty_assertions::assert_eq!(
            cwd_git_stats.apply_results(&layout, &snapshot_fields, results),
            vec![pane_id]
        );
        let mut refreshed_fields = PaneSnapshotFields::default();
        cwd_git_stats.populate_snapshot_fields(&layout, &mut refreshed_fields);
        pretty_assertions::assert_eq!(
            refreshed_fields.git_stats(pane_id).map(|stats| stats.new_files),
            Some(1)
        );

        layout.entries.clear();
        pretty_assertions::assert_eq!(
            cwd_git_stats.prepare_refreshes(&layout, &snapshot_fields, [pane_id]),
            Vec::<CwdGitStatsRefresh>::new()
        );
        let mut removed_fields = PaneSnapshotFields::default();
        cwd_git_stats.populate_snapshot_fields(&layout, &mut removed_fields);
        pretty_assertions::assert_eq!(removed_fields.git_stats(pane_id), None);
        Ok(())
    }

    #[test]
    fn test_cwd_git_stats_when_panes_share_repo_populates_sibling_stats() -> rootcause::Result<()> {
        let temp = self::repo_with_initial_file()?;
        let nested = temp.path().join("src");
        fs::create_dir(&nested).context("failed to create nested repo dir")?;
        let mut layout = self::layout_for_cwd(nested.to_string_lossy())?;
        let first_pane = match &layout.entries[0].pane_tree {
            PaneTree::Pane(pane) => pane.clone(),
            PaneTree::Split { .. } => return Err(rootcause::report!("expected initial single pane layout")),
        };
        let second_pane_id = PaneId::new(2)?;
        let mut second_pane = first_pane.clone();
        second_pane.id = second_pane_id;
        second_pane.cwd = temp.path().to_string_lossy().into_owned();
        layout.entries[0].pane_tree = PaneTree::Split {
            axis: PaneSplitAxis::Vertical,
            first_ratio: PaneSplitRatio::new(500)?,
            first: Box::new(PaneTree::Pane(first_pane)),
            second: Box::new(PaneTree::Pane(second_pane)),
        };
        let mut cwd_git_stats = CwdGitStats::default();
        let snapshot_fields = PaneSnapshotFields::default();
        let first_pane_id = PaneId::new(1)?;

        let results = self::git_stats_results_for_refreshes(cwd_git_stats.prepare_refreshes(
            &layout,
            &snapshot_fields,
            [first_pane_id],
        ));

        pretty_assertions::assert_eq!(
            cwd_git_stats.apply_results(&layout, &snapshot_fields, results),
            vec![first_pane_id, second_pane_id],
        );
        let mut fields = PaneSnapshotFields::default();
        cwd_git_stats.populate_snapshot_fields(&layout, &mut fields);
        pretty_assertions::assert_eq!(fields.git_stats(first_pane_id), Some(GitStats::default()));
        pretty_assertions::assert_eq!(fields.git_stats(second_pane_id), Some(GitStats::default()));
        Ok(())
    }

    #[test]
    fn test_cwd_git_stats_when_submitter_leaves_shared_repo_refreshes_remaining_pane() -> rootcause::Result<()> {
        let temp = self::repo_with_initial_file()?;
        let old_cwd = temp.path().to_string_lossy().into_owned();
        let new_cwd = TempDir::new().context("failed to create non-git cwd")?;
        let mut layout = self::layout_for_cwd(&old_cwd)?;
        let first_pane = match &layout.entries[0].pane_tree {
            PaneTree::Pane(pane) => pane.clone(),
            PaneTree::Split { .. } => return Err(rootcause::report!("expected initial single pane layout")),
        };
        let first_pane_id = first_pane.id;
        let second_pane_id = PaneId::new(2)?;
        let mut second_pane = first_pane.clone();
        second_pane.id = second_pane_id;
        layout.entries[0].pane_tree = PaneTree::Split {
            axis: PaneSplitAxis::Vertical,
            first_ratio: PaneSplitRatio::new(500)?,
            first: Box::new(PaneTree::Pane(first_pane)),
            second: Box::new(PaneTree::Pane(second_pane)),
        };
        let mut cwd_git_stats = CwdGitStats::default();
        let snapshot_fields = PaneSnapshotFields::default();
        let results = self::git_stats_results_for_refreshes(cwd_git_stats.prepare_refreshes(
            &layout,
            &snapshot_fields,
            [first_pane_id],
        ));
        drop(cwd_git_stats.apply_results(&layout, &snapshot_fields, results));
        cwd_git_stats.mark_shell_submit_cwd(first_pane_id, old_cwd);
        layout.sync_terminal_titles(&[(first_pane_id, Some(new_cwd.path().to_string_lossy().into_owned()))]);
        fs::write(temp.path().join("untracked.txt"), "untracked\n").context("failed to write untracked file")?;

        let results = self::git_stats_results_for_refreshes(cwd_git_stats.take_ready_submit_refreshes(
            &layout,
            &[(first_pane_id, Some(new_cwd.path().to_string_lossy().into_owned()))],
        ));

        pretty_assertions::assert_eq!(
            cwd_git_stats.apply_results(&layout, &snapshot_fields, results),
            vec![second_pane_id],
        );
        let mut fields = PaneSnapshotFields::default();
        cwd_git_stats.populate_snapshot_fields(&layout, &mut fields);
        pretty_assertions::assert_eq!(fields.git_stats(first_pane_id), None);
        pretty_assertions::assert_eq!(fields.git_stats(second_pane_id).map(|stats| stats.new_files), Some(1));
        Ok(())
    }

    #[test]
    fn test_cwd_git_stats_when_stale_result_arrives_discards_it() -> rootcause::Result<()> {
        let temp = self::repo_with_initial_file()?;
        let layout = self::layout_for_cwd(temp.path().to_string_lossy())?;
        let pane_id = PaneId::new(1)?;
        let mut cwd_git_stats = CwdGitStats::default();
        let snapshot_fields = PaneSnapshotFields::default();
        let stale_generation = cwd_git_stats.prepare_refreshes(&layout, &snapshot_fields, [pane_id])[0].generation;
        let current_generation = cwd_git_stats.prepare_refreshes(&layout, &snapshot_fields, [pane_id])[0].generation;

        let stale_result = CwdGitStatsResult {
            cwd: temp.path().to_string_lossy().into_owned(),
            generation: stale_generation,
            snapshot: Some(self::git_stats_snapshot(
                temp.path(),
                GitStats {
                    deletions: 1,
                    insertions: 1,
                    new_files: 1,
                },
            )),
        };
        pretty_assertions::assert_eq!(
            cwd_git_stats.apply_results(&layout, &snapshot_fields, vec![stale_result]),
            Vec::<PaneId>::new()
        );

        let current_result = CwdGitStatsResult {
            cwd: temp.path().to_string_lossy().into_owned(),
            generation: current_generation,
            snapshot: Some(self::git_stats_snapshot(temp.path(), GitStats::default())),
        };
        pretty_assertions::assert_eq!(
            cwd_git_stats.apply_results(&layout, &snapshot_fields, vec![current_result]),
            vec![pane_id],
        );
        Ok(())
    }

    #[test]
    fn test_cwd_git_stats_when_result_is_accepted_drops_completed_generation() -> rootcause::Result<()> {
        let temp = self::repo_with_initial_file()?;
        let layout = self::layout_for_cwd(temp.path().to_string_lossy())?;
        let pane_id = PaneId::new(1)?;
        let mut cwd_git_stats = CwdGitStats::default();
        let snapshot_fields = PaneSnapshotFields::default();
        let refresh = cwd_git_stats
            .prepare_refreshes(&layout, &snapshot_fields, [pane_id])
            .into_iter()
            .next()
            .ok_or_else(|| rootcause::report!("expected git stats refresh"))?;
        assert2::assert!(cwd_git_stats.latest_generation_by_cwd.contains_key(&refresh.cwd));

        drop(cwd_git_stats.apply_results(
            &layout,
            &snapshot_fields,
            vec![CwdGitStatsResult {
                cwd: refresh.cwd.clone(),
                generation: refresh.generation,
                snapshot: Some(self::git_stats_snapshot(temp.path(), GitStats::default())),
            }],
        ));

        assert2::assert!(!cwd_git_stats.latest_generation_by_cwd.contains_key(&refresh.cwd));
        pretty_assertions::assert_eq!(
            cwd_git_stats.apply_results(
                &layout,
                &snapshot_fields,
                vec![CwdGitStatsResult {
                    cwd: refresh.cwd,
                    generation: refresh.generation,
                    snapshot: Some(self::git_stats_snapshot(
                        temp.path(),
                        GitStats {
                            deletions: 1,
                            insertions: 1,
                            new_files: 1,
                        },
                    )),
                }],
            ),
            Vec::<PaneId>::new()
        );
        Ok(())
    }

    #[test]
    fn test_cwd_git_stats_when_non_git_result_was_absent_does_not_mark_changed() -> rootcause::Result<()> {
        let temp = TempDir::new().context("failed to create non-git temp dir")?;
        let layout = self::layout_for_cwd(temp.path().to_string_lossy())?;
        let pane_id = PaneId::new(1)?;
        let mut cwd_git_stats = CwdGitStats::default();
        let snapshot_fields = PaneSnapshotFields::default();
        let generation = cwd_git_stats.prepare_refreshes(&layout, &snapshot_fields, [pane_id])[0].generation;

        let result = CwdGitStatsResult {
            cwd: temp.path().to_string_lossy().into_owned(),
            generation,
            snapshot: None,
        };

        pretty_assertions::assert_eq!(
            cwd_git_stats.apply_results(&layout, &snapshot_fields, vec![result]),
            Vec::<PaneId>::new()
        );
        Ok(())
    }

    #[test]
    fn test_cwd_git_stats_requester_when_wakeup_is_full_coalesces_same_cwd_refreshes() -> rootcause::Result<()> {
        let pending = Arc::new(Mutex::new(BTreeMap::new()));
        let (wake_sender, mut wake_receiver) = mpsc::channel(1);
        let requester = CwdGitStatsRequester {
            pending: Arc::clone(&pending),
            wake_sender,
        };

        requester.request_refreshes(vec![CwdGitStatsRefresh {
            cwd: "/repo".to_owned(),
            generation: 1,
        }])?;
        requester.request_refreshes(vec![CwdGitStatsRefresh {
            cwd: "/repo".to_owned(),
            generation: 2,
        }])?;

        assert2::assert!(matches!(wake_receiver.try_recv(), Ok(())));
        assert2::assert!(matches!(
            wake_receiver.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));
        let (cwd, generation) = {
            let pending = self::lock_pending_refreshes(&pending)?;
            let refresh = pending
                .get("/repo")
                .ok_or_else(|| rootcause::report!("expected pending git stats refresh"))?;
            let cwd = refresh.cwd.clone();
            let generation = refresh.generation;
            drop(pending);
            (cwd, generation)
        };
        pretty_assertions::assert_eq!(cwd, "/repo");
        pretty_assertions::assert_eq!(generation, 2);
        Ok(())
    }

    #[test]
    fn test_cwd_git_stats_requester_when_wakeup_is_full_preserves_different_cwd_refreshes() -> rootcause::Result<()> {
        let pending = Arc::new(Mutex::new(BTreeMap::new()));
        let (wake_sender, mut wake_receiver) = mpsc::channel(1);
        let requester = CwdGitStatsRequester {
            pending: Arc::clone(&pending),
            wake_sender,
        };

        requester.request_refreshes(vec![CwdGitStatsRefresh {
            cwd: "/old".to_owned(),
            generation: 1,
        }])?;
        requester.request_refreshes(vec![CwdGitStatsRefresh {
            cwd: "/new".to_owned(),
            generation: 2,
        }])?;

        assert2::assert!(matches!(wake_receiver.try_recv(), Ok(())));
        assert2::assert!(matches!(
            wake_receiver.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));
        let pending = self::lock_pending_refreshes(&pending)?;
        pretty_assertions::assert_eq!(
            pending.keys().cloned().collect::<Vec<_>>(),
            vec!["/new".to_owned(), "/old".to_owned()],
        );
        Ok(())
    }

    #[test]
    fn test_group_refreshes_by_repo_workdir_when_cwds_share_repo_keeps_one_repo_group() -> rootcause::Result<()> {
        let temp = self::repo_with_initial_file()?;
        let nested = temp.path().join("src");
        fs::create_dir(&nested).context("failed to create nested repo dir")?;
        let repo_root = temp.path().to_string_lossy().into_owned();
        let nested_cwd = nested.to_string_lossy().into_owned();

        let (non_git_results, repo_refreshes) = self::group_refreshes_by_repo_workdir(
            vec![
                CwdGitStatsRefresh {
                    cwd: repo_root.clone(),
                    generation: 1,
                },
                CwdGitStatsRefresh {
                    cwd: nested_cwd.clone(),
                    generation: 2,
                },
            ],
            None,
        );

        pretty_assertions::assert_eq!(non_git_results, Vec::<CwdGitStatsResult>::new());
        pretty_assertions::assert_eq!(
            repo_refreshes.keys().cloned().collect::<Vec<_>>(),
            vec![path_key(temp.path())]
        );
        let group = repo_refreshes
            .get(&path_key(temp.path()))
            .ok_or_else(|| rootcause::report!("expected same-repo git stats refresh group"))?;
        pretty_assertions::assert_eq!(
            group
                .refreshes
                .iter()
                .map(|refresh| refresh.cwd.clone())
                .collect::<Vec<_>>(),
            vec![repo_root, nested_cwd],
        );
        Ok(())
    }

    #[test]
    fn test_cwd_git_stats_when_shell_submit_reaches_prompt_takes_pending_refresh() -> rootcause::Result<()> {
        let layout = self::layout_for_cwd("/repo")?;
        let pane_id = PaneId::new(1)?;
        let mut cwd_git_stats = CwdGitStats::default();

        cwd_git_stats.mark_shell_submit_cwd(pane_id, "/repo".to_owned());

        pretty_assertions::assert_eq!(
            self::refresh_cwds(
                cwd_git_stats.take_ready_submit_refreshes(&layout, &[(pane_id, Some("/repo".to_owned()))])
            ),
            vec!["/repo"]
        );
        pretty_assertions::assert_eq!(
            cwd_git_stats.take_ready_submit_refreshes(&layout, &[(pane_id, Some("/repo".to_owned()))]),
            Vec::<CwdGitStatsRefresh>::new()
        );
        Ok(())
    }

    #[test]
    fn test_cwd_git_stats_when_shell_submit_changes_cwd_refreshes_submit_cwd() -> rootcause::Result<()> {
        let old_cwd = TempDir::new().context("failed to create old cwd")?;
        let new_cwd = TempDir::new().context("failed to create new cwd")?;
        let layout = self::layout_for_cwd(new_cwd.path().to_string_lossy())?;
        let pane_id = PaneId::new(1)?;
        let mut cwd_git_stats = CwdGitStats::default();

        cwd_git_stats.mark_shell_submit_cwd(pane_id, old_cwd.path().to_string_lossy().into_owned());

        pretty_assertions::assert_eq!(
            self::refresh_cwds(cwd_git_stats.take_ready_submit_refreshes(
                &layout,
                &[(pane_id, Some(new_cwd.path().to_string_lossy().into_owned()))],
            )),
            vec![old_cwd.path().to_string_lossy().into_owned()],
        );
        Ok(())
    }

    #[test]
    fn test_cwd_git_stats_when_shell_submit_reports_cmd_title_keeps_pending_refresh() -> rootcause::Result<()> {
        let layout = self::layout_for_cwd("/repo")?;
        let pane_id = PaneId::new(1)?;
        let mut cwd_git_stats = CwdGitStats::default();

        cwd_git_stats.mark_shell_submit_cwd(pane_id, "/repo".to_owned());

        pretty_assertions::assert_eq!(
            cwd_git_stats.take_ready_submit_refreshes(&layout, &[(pane_id, Some("cargo test".to_owned()))]),
            Vec::<CwdGitStatsRefresh>::new()
        );
        pretty_assertions::assert_eq!(
            self::refresh_cwds(
                cwd_git_stats.take_ready_submit_refreshes(&layout, &[(pane_id, Some("/repo".to_owned()))])
            ),
            vec!["/repo"]
        );
        Ok(())
    }

    fn repo_with_initial_file() -> rootcause::Result<TempDir> {
        let temp = TempDir::new().context("failed to create temp git repo dir")?;
        self::init_repo_with_initial_file(temp.path())?;
        Ok(temp)
    }

    fn layout_for_cwd(cwd: impl Into<String>) -> rootcause::Result<SessionLayout> {
        let session = SessionName::default();
        SessionLayout::initial(
            &session,
            SessionMetadata {
                cmd_label: "sh".to_owned(),
                cwd: cwd.into(),
                started_at: 1,
            },
        )
    }

    fn init_repo_with_initial_file(path: &Path) -> rootcause::Result<()> {
        let repo = Repository::init(path).context("failed to init temp git repo")?;
        fs::write(path.join("tracked.txt"), "old\n").context("failed to write tracked file")?;
        let mut index = repo.index().context("failed to open temp git repo index")?;
        index
            .add_path(Path::new("tracked.txt"))
            .context("failed to stage tracked file")?;
        index.write().context("failed to write temp git index")?;
        let tree_id = index.write_tree().context("failed to write temp git tree")?;
        let tree = repo.find_tree(tree_id).context("failed to find temp git tree")?;
        let signature = Signature::now("muxr test", "muxr@example.invalid").context("failed to create signature")?;
        repo.commit(Some("HEAD"), &signature, &signature, "initial", &tree, &[])
            .context("failed to commit temp git tree")?;
        Ok(())
    }

    fn git_stats_snapshot(repo_workdir: &Path, stats: GitStats) -> RepoGitStatsSnapshot {
        RepoGitStatsSnapshot {
            repo_workdir: path_key(repo_workdir),
            stats,
        }
    }

    fn repo_git_stats_snapshot_for_path(path: &Path) -> Option<RepoGitStatsSnapshot> {
        self::repo_git_stats_snapshot_for_path_with_home(path, None)
    }

    fn repo_git_stats_snapshot_for_path_with_home(path: &Path, home: Option<&Path>) -> Option<RepoGitStatsSnapshot> {
        self::git_stats_results_for_refreshes_with_home(
            vec![CwdGitStatsRefresh {
                cwd: path.to_string_lossy().into_owned(),
                generation: 1,
            }],
            home,
        )
        .into_iter()
        .next()?
        .snapshot
    }

    fn refresh_cwds(refreshes: Vec<CwdGitStatsRefresh>) -> Vec<String> {
        refreshes.into_iter().map(|refresh| refresh.cwd).collect()
    }
}
