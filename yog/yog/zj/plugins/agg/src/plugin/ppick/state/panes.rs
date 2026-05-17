use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;

use zellij_tile::prelude::PaneManifest;
use zellij_tile::prelude::TabInfo;

use crate::plugin::ppick::entry::PaneEntry;
use crate::plugin::ppick::state::PaneObservation;
use crate::plugin::ppick::state::PpickState;

impl PpickState {
    pub fn update_panes(
        &mut self,
        manifest: &PaneManifest,
        resolve_pane_cwd: impl FnMut(u32) -> Option<PathBuf>,
        resolve_pane_command: impl FnMut(u32) -> Option<Vec<String>>,
    ) -> bool {
        let observations = self.pane_observations(manifest, resolve_pane_cwd, resolve_pane_command);
        self.update_panes_from_observations(&observations)
    }

    pub fn update_tabs(&mut self, mut tabs: Vec<TabInfo>) -> bool {
        let display_area_changed = self.update_floating_display_area(&tabs);
        tabs.sort_by_key(|tab| tab.position);
        let mut changed = self.all_tabs != tabs;
        self.all_tabs = tabs;
        for entry in &mut self.pane_entries {
            changed |= entry.apply_tab_metadata(&self.all_tabs);
        }
        let mut previous_order = Vec::with_capacity(self.pane_entries.len());
        for entry in &self.pane_entries {
            previous_order.push(entry.pane_id);
        }
        crate::plugin::ppick::entry::sort_by_tab_order(&mut self.pane_entries, &self.all_tabs);
        let ordered_pane_ids = self.pane_entries.iter().map(|entry| entry.pane_id);
        let order_changed = !ordered_pane_ids.eq(previous_order);
        if changed || order_changed {
            self.mark_filter_dirty();
        }
        let selection_changed = self.select_initial_focus(None);
        changed || order_changed || selection_changed || display_area_changed
    }

    pub fn update_cwd(&mut self, pane_id: u32, cwd: &Path) -> bool {
        let cwd = cwd.to_path_buf();
        let cwd_changed = self.cwds_by_pane.get(&pane_id) != Some(&cwd);
        self.cwds_by_pane.insert(pane_id, cwd.clone());
        if cwd_changed {
            self.git_stat_cwds_to_refresh.insert(cwd.clone());
        }
        let git_stat = self.git_stats_by_cwd.get(&cwd).cloned().unwrap_or_default();
        let Some(entry) = self.pane_entries.iter_mut().find(|entry| entry.pane_id == pane_id) else {
            return cwd_changed;
        };
        let mut changed = entry.apply_cwd(cwd, git_stat);
        if changed {
            self.mark_filter_dirty();
            changed |= self.clamp_selection();
        }
        changed || cwd_changed
    }

    pub fn update_command(&mut self, pane_id: u32, command: &[String]) -> bool {
        self.commands_by_pane.insert(pane_id, command.to_owned());
        let Some(entry) = self.pane_entries.iter_mut().find(|entry| entry.pane_id == pane_id) else {
            return false;
        };
        let mut changed = entry.apply_command(command.to_owned());
        if let Some(snapshot) = self.agent_snapshots_by_pane.get(&pane_id) {
            changed |= entry.apply_agent_snapshot(*snapshot);
        }
        if changed {
            changed |= entry.attach_session(&self.sessions_by_key);
            self.mark_filter_dirty();
            changed |= self.clamp_selection();
        }
        changed
    }

    pub fn remove_pane(&mut self, pane_id: u32) -> bool {
        self.cwds_by_pane.remove(&pane_id);
        self.commands_by_pane.remove(&pane_id);
        self.agent_snapshots_by_pane.remove(&pane_id);
        let old_len = self.pane_entries.len();
        self.pane_entries.retain(|entry| entry.pane_id != pane_id);
        if old_len != self.pane_entries.len() {
            self.mark_filter_dirty();
        }
        let selection_changed = self.clamp_selection();
        old_len != self.pane_entries.len() || selection_changed
    }

    fn pane_observations(
        &self,
        manifest: &PaneManifest,
        mut resolve_pane_cwd: impl FnMut(u32) -> Option<PathBuf>,
        mut resolve_pane_command: impl FnMut(u32) -> Option<Vec<String>>,
    ) -> Vec<PaneObservation> {
        let mut observations = Vec::new();
        for (&tab_position, panes) in &manifest.panes {
            for pane in panes
                .iter()
                .filter(|pane| crate::plugin::pane::is_displayable_terminal_pane(pane))
            {
                let cwd = (!self.cwds_by_pane.contains_key(&pane.id))
                    .then(|| resolve_pane_cwd(pane.id))
                    .flatten();
                let command = (!self.commands_by_pane.contains_key(&pane.id))
                    .then(|| resolve_pane_command(pane.id))
                    .flatten();
                observations.push(PaneObservation {
                    tab_position,
                    pane_id: pane.id,
                    terminal_command_args: pane
                        .terminal_command
                        .as_deref()
                        .map(|command| command.split_whitespace().map(str::to_string).collect()),
                    title_label: crate::plugin::pane::title_label_from_title(&pane.title),
                    is_focused: pane.is_focused,
                    cwd,
                    command,
                });
            }
        }
        observations
    }

    fn update_panes_from_observations(&mut self, observations: &[PaneObservation]) -> bool {
        let mut next_entries = Vec::new();
        let mut live_pane_ids = HashSet::new();
        let mut git_refresh_queued = false;

        for pane in observations {
            live_pane_ids.insert(pane.pane_id);
            if let Some(cwd) = pane.cwd.clone() {
                let cwd_changed = self.cwds_by_pane.get(&pane.pane_id) != Some(&cwd);
                self.cwds_by_pane.insert(pane.pane_id, cwd.clone());
                if cwd_changed {
                    self.git_stat_cwds_to_refresh.insert(cwd);
                    git_refresh_queued = true;
                }
            }
            if let Some(command) = pane.command.clone() {
                self.commands_by_pane.insert(pane.pane_id, command);
            }
            next_entries.push(self.entry_from_observation(pane));
        }

        self.cwds_by_pane.retain(|pane_id, _| live_pane_ids.contains(pane_id));
        self.commands_by_pane
            .retain(|pane_id, _| live_pane_ids.contains(pane_id));
        self.agent_snapshots_by_pane
            .retain(|pane_id, _| live_pane_ids.contains(pane_id));
        crate::plugin::ppick::entry::sort_by_tab_order(&mut next_entries, &self.all_tabs);
        super::sessions::attach_sessions_to_entries(&mut next_entries, &self.sessions_by_key);
        let entries_changed = self.pane_entries != next_entries;
        self.pane_entries = next_entries;
        if entries_changed {
            self.mark_filter_dirty();
        }
        let focused_pane_id = observations
            .iter()
            .find(|pane| pane.is_focused)
            .map(|pane| pane.pane_id);
        let selection_changed = self.select_initial_focus(focused_pane_id);
        entries_changed || selection_changed || git_refresh_queued
    }

    fn entry_from_observation(&self, pane: &PaneObservation) -> PaneEntry {
        let cached_cwd = self.cwds_by_pane.get(&pane.pane_id).cloned();
        let cached_command = self.commands_by_pane.get(&pane.pane_id).cloned();
        let previous = self.pane_entries.iter().find(|entry| entry.pane_id == pane.pane_id);
        let mut entry = PaneEntry::from_observation(pane, cached_cwd, cached_command, &self.all_tabs, previous);
        if let Some(previous) = previous {
            entry.inherit_agent_state(previous);
        }
        if let Some(snapshot) = self.agent_snapshots_by_pane.get(&pane.pane_id) {
            entry.apply_agent_snapshot(*snapshot);
        }
        if let Some(cwd) = entry.cwd.as_ref()
            && let Some(stat) = self.git_stats_by_cwd.get(cwd)
        {
            entry.apply_git_stat(stat.clone());
        }
        entry
    }
}
