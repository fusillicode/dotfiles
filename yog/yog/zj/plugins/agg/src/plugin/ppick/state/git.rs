use std::path::Path;
use std::path::PathBuf;

use agg::GitStat;

use crate::plugin::ppick::state::PpickState;

impl PpickState {
    pub fn take_git_stat_cwds_to_request(&mut self) -> Vec<PathBuf> {
        let mut cwds = self.git_stat_cwds_to_refresh.drain().collect::<Vec<_>>();
        cwds.sort();
        let mut requests = Vec::new();
        for cwd in cwds {
            if self.git_stat_cwds_in_flight.insert(cwd.clone()) {
                requests.push(cwd);
            }
        }
        requests
    }

    pub fn finish_git_stat_request(&mut self, cwd: &Path) {
        self.git_stat_cwds_in_flight.remove(cwd);
    }

    pub fn update_git_stat(&mut self, stat: &GitStat) -> bool {
        let cwd = stat.path.clone();
        let previous = self.git_stats_by_cwd.insert(cwd.clone(), stat.clone());
        let mut changed = previous.as_ref() != Some(stat);
        for entry in &mut self.pane_entries {
            if entry.cwd.as_deref() == Some(cwd.as_path()) {
                changed |= entry.apply_git_stat(stat.clone());
            }
        }
        if changed {
            self.mark_filter_dirty();
            changed |= self.clamp_selection();
        }
        changed
    }
}
