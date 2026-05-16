use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;

use agg::GitStat;
use serde::Deserialize;
use ytil_agents::agent::AgentEventPayload;
use zellij_tile::prelude::BareKey;
use zellij_tile::prelude::FloatingPaneCoordinates;
use zellij_tile::prelude::KeyModifier;
use zellij_tile::prelude::KeyWithModifier;
use zellij_tile::prelude::PaneManifest;
use zellij_tile::prelude::TabInfo;

use crate::plugin::ppick::entry::PaneEntry;
use crate::plugin::ppick::ui::PpickRow;
use crate::plugin::tbar::PaneAgentSnapshot;
use crate::plugin::tbar::StateSnapshotPayload;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PpickMode {
    #[default]
    AllPanes,
    AgentsOnly,
}

impl PpickMode {
    const fn includes_entry(self, entry: &PaneEntry) -> bool {
        match self {
            Self::AllPanes => true,
            Self::AgentsOnly => entry.is_agent_pane(),
        }
    }
}

#[derive(Default)]
pub struct PpickState {
    mode: PpickMode,
    pub home_dir: PathBuf,
    pub query: String,
    selected: usize,
    selected_pane_id: Option<u32>,
    filtered_entry_indices: Vec<usize>,
    filter_ready: bool,
    pane_entries: Vec<PaneEntry>,
    sessions_by_key: HashMap<(String, String), SessionEntry>,
    cwds_by_pane: HashMap<u32, PathBuf>,
    commands_by_pane: HashMap<u32, Vec<String>>,
    agent_snapshots_by_pane: HashMap<u32, PaneAgentSnapshot>,
    git_stats_by_cwd: HashMap<PathBuf, GitStat>,
    git_stat_cwds_to_refresh: HashSet<PathBuf>,
    git_stat_cwds_in_flight: HashSet<PathBuf>,
    all_tabs: Vec<TabInfo>,
    floating_y: Option<String>,
    floating_width: Option<String>,
    floating_height: Option<String>,
    floating_display_rows: Option<usize>,
    floating_display_columns: Option<usize>,
    floating_size_applied: bool,
    initial_focus_by_tab: HashMap<usize, InitialFocus>,
    selection_touched: bool,
}

#[derive(Clone, Copy)]
struct InitialFocus {
    pane_id: u32,
    seq: u64,
}

#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
pub enum PpickAction {
    None,
    Redraw,
    Close,
    Focus(u32),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct SessionEntry {
    pub agent: String,
    pub workspace: PathBuf,
    pub session_id: String,
    #[serde(default)]
    pub summary: Option<String>,
    pub display: String,
    pub search: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaneObservation {
    pub tab_position: usize,
    pub pane_id: u32,
    pub terminal_command_args: Option<Vec<String>>,
    pub title_label: Option<String>,
    pub is_focused: bool,
    cwd: Option<PathBuf>,
    command: Option<Vec<String>>,
}

impl PpickState {
    pub fn new(mode: PpickMode) -> Self {
        Self {
            mode,
            ..Default::default()
        }
    }

    pub fn set_floating_coordinates(&mut self, y: Option<String>, width: Option<String>, height: Option<String>) {
        self.floating_y = y;
        self.floating_width = width;
        self.floating_height = height;
        self.floating_size_applied = false;
    }

    pub fn take_floating_coordinates(&mut self) -> Option<FloatingPaneCoordinates> {
        if self.floating_size_applied {
            return None;
        }
        let coordinates = match (self.floating_display_rows, self.floating_display_columns) {
            (Some(display_rows), Some(display_columns)) => {
                self.centered_floating_coordinates(display_rows, display_columns)
            }
            _ => FloatingPaneCoordinates::new(
                None,
                self.floating_y.clone(),
                self.floating_width.clone(),
                self.floating_height.clone(),
                None,
                Some(false),
            ),
        }?;
        self.floating_size_applied = true;
        Some(coordinates)
    }

    pub fn set_initial_focus_pane(&mut self, tab_id: usize, pane_id: u32, seq: u64) -> bool {
        if self.selection_touched {
            return false;
        }
        if self
            .initial_focus_by_tab
            .get(&tab_id)
            .is_some_and(|focus| seq < focus.seq)
        {
            return false;
        }
        self.initial_focus_by_tab.insert(tab_id, InitialFocus { pane_id, seq });
        self.select_initial_focus(None)
    }

    pub fn apply_state_snapshot(&mut self, snapshot: &StateSnapshotPayload) -> bool {
        let mut changed = snapshot
            .focused_pane_id
            .is_some_and(|pane_id| self.set_initial_focus_pane(snapshot.tab_id, pane_id, snapshot.seq));
        changed |= self.update_agent_snapshots(&snapshot.pane_agents);
        changed
    }

    pub fn update_panes(
        &mut self,
        manifest: &PaneManifest,
        resolve_pane_cwd: impl FnMut(u32) -> Option<PathBuf>,
        resolve_pane_command: impl FnMut(u32) -> Option<Vec<String>>,
    ) -> bool {
        let observations = self.pane_observations(manifest, resolve_pane_cwd, resolve_pane_command);
        self.update_panes_from_observations(&observations)
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

    fn update_floating_display_area(&mut self, tabs: &[TabInfo]) -> bool {
        let Some(active_tab) = tabs.iter().find(|tab| tab.active) else {
            return false;
        };
        if active_tab.display_area_rows == 0 || active_tab.display_area_columns == 0 {
            return false;
        }

        let display_area_changed = self.floating_display_rows != Some(active_tab.display_area_rows)
            || self.floating_display_columns != Some(active_tab.display_area_columns);
        if display_area_changed {
            self.floating_display_rows = Some(active_tab.display_area_rows);
            self.floating_display_columns = Some(active_tab.display_area_columns);
            self.floating_size_applied = false;
        }
        display_area_changed
    }

    fn centered_floating_coordinates(
        &self,
        display_rows: usize,
        display_columns: usize,
    ) -> Option<FloatingPaneCoordinates> {
        let width = fixed_cells(self.floating_width.as_deref(), display_columns);
        let height = fixed_cells(self.floating_height.as_deref(), display_rows);
        let x = width.map(|width| display_columns.saturating_sub(width) / 2);
        let y = fixed_cells(self.floating_y.as_deref(), display_rows);
        FloatingPaneCoordinates::new(
            x.map(|x| x.to_string()),
            y.map(|y| y.to_string()),
            width.map(|width| width.to_string()),
            height.map(|height| height.to_string()),
            None,
            Some(false),
        )
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
        crate::plugin::ppick::state::attach_sessions_to_entries(&mut next_entries, &self.sessions_by_key);
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

    pub fn update_sessions(&mut self, session_entries: Vec<SessionEntry>) -> bool {
        let next_sessions_by_key = crate::plugin::ppick::state::index_session_entries(session_entries);
        let sessions_changed = self.sessions_by_key != next_sessions_by_key;
        self.sessions_by_key = next_sessions_by_key;
        let entries_changed =
            crate::plugin::ppick::state::attach_sessions_to_entries(&mut self.pane_entries, &self.sessions_by_key);
        if entries_changed {
            self.mark_filter_dirty();
        }
        let selection_changed = self.clamp_selection();
        sessions_changed || entries_changed || selection_changed
    }

    pub fn update_agent(&mut self, event: &AgentEventPayload) -> bool {
        let Some(entry) = self
            .pane_entries
            .iter_mut()
            .find(|entry| entry.pane_id == event.pane_id)
        else {
            return false;
        };
        let mut changed = entry.apply_agent_event(event);
        self.agent_snapshots_by_pane.remove(&event.pane_id);
        if changed {
            self.mark_filter_dirty();
            changed |= self.clamp_selection();
        }
        changed
    }

    pub fn update_agent_snapshots(&mut self, snapshots: &[PaneAgentSnapshot]) -> bool {
        let mut changed = false;
        for snapshot in snapshots {
            if self.agent_snapshots_by_pane.get(&snapshot.pane_id) != Some(snapshot) {
                self.agent_snapshots_by_pane.insert(snapshot.pane_id, *snapshot);
            }
            if let Some(entry) = self
                .pane_entries
                .iter_mut()
                .find(|entry| entry.pane_id == snapshot.pane_id)
            {
                changed |= entry.apply_agent_snapshot(*snapshot);
            }
        }
        if changed {
            self.mark_filter_dirty();
            changed |= self.clamp_selection();
        }
        changed
    }

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

    pub fn handle_key(&mut self, key: &KeyWithModifier) -> PpickAction {
        match key.bare_key {
            BareKey::Esc if key.has_no_modifiers() => PpickAction::Close,
            BareKey::Enter if key.has_no_modifiers() => {
                self.ensure_filter();
                self.selected_entry()
                    .map_or(PpickAction::None, |entry| PpickAction::Focus(entry.pane_id))
            }
            BareKey::Backspace if key.has_no_modifiers() => {
                if self.query.pop().is_none() {
                    return PpickAction::None;
                }
                self.selection_touched = true;
                self.mark_filter_dirty();
                self.clamp_selection();
                PpickAction::Redraw
            }
            BareKey::Down if key.has_no_modifiers() => self.select_next(),
            BareKey::Char('n') if key.has_only_modifiers(&[KeyModifier::Ctrl]) => self.select_next(),
            BareKey::Up if key.has_no_modifiers() => self.select_previous(),
            BareKey::Char('p') if key.has_only_modifiers(&[KeyModifier::Ctrl]) => self.select_previous(),
            BareKey::Char(c)
                if !c.is_control() && (key.has_no_modifiers() || key.has_only_modifiers(&[KeyModifier::Shift])) =>
            {
                self.query.push(c);
                self.selection_touched = true;
                self.mark_filter_dirty();
                self.clamp_selection();
                PpickAction::Redraw
            }
            BareKey::PageDown
            | BareKey::PageUp
            | BareKey::Left
            | BareKey::Right
            | BareKey::Home
            | BareKey::End
            | BareKey::Delete
            | BareKey::Insert
            | BareKey::F(_)
            | BareKey::Tab
            | BareKey::CapsLock
            | BareKey::ScrollLock
            | BareKey::NumLock
            | BareKey::PrintScreen
            | BareKey::Pause
            | BareKey::Menu
            | BareKey::Esc
            | BareKey::Enter
            | BareKey::Backspace
            | BareKey::Down
            | BareKey::Up
            | BareKey::Char(_) => PpickAction::None,
        }
    }

    pub fn visible_frame(&mut self, capacity: usize) -> Vec<PpickRow> {
        self.ensure_filter();
        if capacity == 0 {
            return Vec::new();
        }
        let start = self.selected.saturating_add(1).saturating_sub(capacity);
        self.filtered_entry_indices
            .iter()
            .enumerate()
            .skip(start)
            .take(capacity)
            .filter_map(|(idx, entry_idx)| {
                self.pane_entries
                    .get(*entry_idx)
                    .map(|entry| entry.row(idx == self.selected, &self.home_dir))
            })
            .collect()
    }

    pub fn should_close_empty_picker(&mut self) -> bool {
        if self.mode != PpickMode::AgentsOnly || !self.query.is_empty() {
            return false;
        }
        self.ensure_filter();
        self.filtered_entry_indices.is_empty()
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

    fn filtered_entries(&self) -> impl Iterator<Item = (usize, &PaneEntry)> {
        let query = self.query.trim().to_ascii_lowercase();
        let mode = self.mode;
        self.pane_entries
            .iter()
            .enumerate()
            .filter(move |(_, entry)| mode.includes_entry(entry))
            .filter(move |(_, entry)| entry.matches_normalized_query(&query))
    }

    fn select_next(&mut self) -> PpickAction {
        self.ensure_filter();
        let filtered_len = self.filtered_entry_indices.len();
        if filtered_len <= 1 {
            return PpickAction::None;
        }
        self.selection_touched = true;
        let selected = self
            .selected
            .checked_add(1)
            .filter(|next| *next < filtered_len)
            .unwrap_or(0);
        self.set_selected(selected);
        PpickAction::Redraw
    }

    fn select_previous(&mut self) -> PpickAction {
        self.ensure_filter();
        let filtered_len = self.filtered_entry_indices.len();
        if filtered_len <= 1 {
            return PpickAction::None;
        }
        self.selection_touched = true;
        let selected = self
            .selected
            .checked_sub(1)
            .unwrap_or_else(|| filtered_len.saturating_sub(1));
        self.set_selected(selected);
        PpickAction::Redraw
    }

    fn select_initial_focus(&mut self, observed_focused_pane_id: Option<u32>) -> bool {
        let old_selected = self.selected;
        let old_selected_pane_id = self.selected_pane_id;
        self.ensure_filter();
        if self.selection_touched || !self.query.is_empty() {
            self.clamp_selection();
            return old_selected != self.selected || old_selected_pane_id != self.selected_pane_id;
        }

        let active_tab = self.all_tabs.iter().find(|tab| tab.active);
        let focus_pane_id = active_tab
            .and_then(|tab| self.initial_focus_by_tab.get(&tab.tab_id))
            .map(|focus| focus.pane_id)
            .or(observed_focused_pane_id);
        let selected = focus_pane_id.and_then(|focus_pane_id| self.filtered_position_for_pane(focus_pane_id));
        let selected = selected.or_else(|| {
            let active_tab = active_tab?;
            self.filtered_entry_indices.iter().position(|entry_idx| {
                self.pane_entries
                    .get(*entry_idx)
                    .is_some_and(|entry| entry.tab_position == active_tab.position)
            })
        });
        if let Some(selected) = selected {
            self.set_selected(selected);
        } else {
            self.clamp_selection();
        }
        old_selected != self.selected || old_selected_pane_id != self.selected_pane_id
    }

    fn clamp_selection(&mut self) -> bool {
        let old_selected = self.selected;
        let old_selected_pane_id = self.selected_pane_id;
        self.ensure_filter();
        let filtered_len = self.filtered_entry_indices.len();
        if filtered_len == 0 {
            self.set_selected(0);
        } else if self.selected >= filtered_len {
            self.set_selected(filtered_len.saturating_sub(1));
        } else {
            self.set_selected(self.selected);
        }
        old_selected != self.selected || old_selected_pane_id != self.selected_pane_id
    }

    const fn mark_filter_dirty(&mut self) {
        self.filter_ready = false;
    }

    fn ensure_filter(&mut self) -> bool {
        if self.filter_ready {
            return false;
        }

        let old_indices = self.filtered_entry_indices.clone();
        let old_selected = self.selected;
        let old_selected_pane_id = self.selected_pane_id;
        let selected_pane_id = self
            .selected_pane_id
            .or_else(|| self.selected_entry().map(|entry| entry.pane_id));

        self.filtered_entry_indices = self.filtered_entries().map(|(idx, _)| idx).collect();
        if let Some(selected) = selected_pane_id.and_then(|pane_id| self.filtered_position_for_pane(pane_id)) {
            self.selected = selected;
        } else if self.filtered_entry_indices.is_empty() {
            self.selected = 0;
        } else if self.selected >= self.filtered_entry_indices.len() {
            self.selected = self.filtered_entry_indices.len().saturating_sub(1);
        }
        self.refresh_selected_pane_id();
        self.filter_ready = true;

        old_indices != self.filtered_entry_indices
            || old_selected != self.selected
            || old_selected_pane_id != self.selected_pane_id
    }

    fn set_selected(&mut self, selected: usize) {
        self.selected = selected;
        self.refresh_selected_pane_id();
    }

    fn selected_entry(&self) -> Option<&PaneEntry> {
        self.filtered_entry_indices
            .get(self.selected)
            .and_then(|entry_idx| self.pane_entries.get(*entry_idx))
    }

    fn refresh_selected_pane_id(&mut self) {
        self.selected_pane_id = self.selected_entry().map(|entry| entry.pane_id);
    }

    fn filtered_position_for_pane(&self, pane_id: u32) -> Option<usize> {
        self.filtered_entry_indices.iter().position(|entry_idx| {
            self.pane_entries
                .get(*entry_idx)
                .is_some_and(|entry| entry.pane_id == pane_id)
        })
    }
}

fn fixed_cells(value: Option<&str>, total: usize) -> Option<usize> {
    let value = value?;
    if let Some(percent) = value.strip_suffix('%') {
        let percent = percent.parse::<usize>().ok()?;
        if percent > 100 {
            return None;
        }
        Some(total.saturating_mul(percent) / 100)
    } else {
        value.parse::<usize>().ok()
    }
}

fn attach_sessions_to_entries(
    pane_entries: &mut [PaneEntry],
    sessions_by_key: &HashMap<(String, String), SessionEntry>,
) -> bool {
    let mut changed = false;
    for entry in pane_entries {
        changed |= entry.attach_session(sessions_by_key);
    }
    changed
}

fn index_session_entries(session_entries: Vec<SessionEntry>) -> HashMap<(String, String), SessionEntry> {
    let mut sessions_by_key = HashMap::new();
    for session in session_entries {
        sessions_by_key.insert((session.agent.clone(), session.session_id.clone()), session);
    }
    sessions_by_key
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::collections::BTreeSet;
    use std::path::PathBuf;

    use pretty_assertions::assert_eq;
    use ytil_agents::agent::Agent;
    use ytil_agents::agent::AgentEventKind;
    use ytil_agents::agent::AgentEventPayload;
    use zellij_tile::prelude::BareKey;
    use zellij_tile::prelude::KeyModifier;
    use zellij_tile::prelude::KeyWithModifier;
    use zellij_tile::prelude::PaneInfo;
    use zellij_tile::prelude::PaneManifest;
    use zellij_tile::prelude::TabInfo;

    use super::*;

    fn key(bare_key: BareKey) -> KeyWithModifier {
        KeyWithModifier::new(bare_key)
    }

    fn terminal_pane_with_command(id: u32, command: &str) -> PaneInfo {
        PaneInfo {
            id,
            terminal_command: Some(command.to_string()),
            ..Default::default()
        }
    }

    fn update_panes(
        state: &mut PpickState,
        manifest: &PaneManifest,
        resolve_pane_cwd: impl FnMut(u32) -> Option<PathBuf>,
        resolve_pane_command: impl FnMut(u32) -> Option<Vec<String>>,
    ) -> bool {
        state.update_panes(manifest, resolve_pane_cwd, resolve_pane_command)
    }

    fn frame(state: &mut PpickState) -> Vec<PpickRow> {
        state.mark_filter_dirty();
        state.visible_frame(usize::MAX)
    }

    fn session_entry(agent: &str, workspace: &str, session_id: &str, search: &str, updated_at: &str) -> SessionEntry {
        SessionEntry {
            agent: agent.to_string(),
            workspace: PathBuf::from(workspace),
            session_id: session_id.to_string(),
            summary: Some(format!("{agent} summary")),
            display: format!("{agent} {workspace} {session_id}"),
            search: search.to_string(),
            updated_at: updated_at.to_string(),
        }
    }

    #[test]
    fn test_take_floating_coordinates_centers_inside_active_display_area() {
        let mut state = PpickState::default();
        state.set_floating_coordinates(
            Some(String::from("2")),
            Some(String::from("68%")),
            Some(String::from("45%")),
        );
        let _ = state.update_tabs(vec![TabInfo {
            active: true,
            display_area_rows: 100,
            display_area_columns: 320,
            ..Default::default()
        }]);

        assert_eq!(
            state.take_floating_coordinates(),
            FloatingPaneCoordinates::new(
                Some(String::from("51")),
                Some(String::from("2")),
                Some(String::from("217")),
                Some(String::from("45")),
                None,
                Some(false),
            )
        );
        assert_eq!(state.take_floating_coordinates(), None);
    }

    #[test]
    fn test_take_floating_coordinates_reapplies_after_display_area_changes() {
        let mut state = PpickState::default();
        state.set_floating_coordinates(
            Some(String::from("0")),
            Some(String::from("68%")),
            Some(String::from("45%")),
        );
        let _ = state.update_tabs(vec![TabInfo {
            active: true,
            display_area_rows: 100,
            display_area_columns: 320,
            ..Default::default()
        }]);
        let _ = state.take_floating_coordinates();

        let _ = state.update_tabs(vec![TabInfo {
            active: true,
            display_area_rows: 100,
            display_area_columns: 200,
            ..Default::default()
        }]);

        assert_eq!(
            state.take_floating_coordinates(),
            FloatingPaneCoordinates::new(
                Some(String::from("32")),
                Some(String::from("0")),
                Some(String::from("136")),
                Some(String::from("45")),
                None,
                Some(false),
            )
        );
    }

    #[test]
    fn test_update_panes_includes_open_terminal_panes_and_excludes_plugins_and_suppressed_panes() {
        let mut state = PpickState::default();
        let _ = state.update_tabs(vec![
            TabInfo {
                tab_id: 10,
                position: 0,
                name: "first".to_string(),
                ..Default::default()
            },
            TabInfo {
                tab_id: 20,
                position: 1,
                name: "second".to_string(),
                ..Default::default()
            },
        ]);
        let manifest = PaneManifest {
            panes: [
                (
                    0,
                    vec![
                        PaneInfo {
                            id: 7,
                            is_plugin: true,
                            ..Default::default()
                        },
                        PaneInfo {
                            id: 9,
                            is_suppressed: true,
                            ..terminal_pane_with_command(9, "zsh")
                        },
                        PaneInfo {
                            id: 11,
                            exited: true,
                            ..terminal_pane_with_command(11, "nvim")
                        },
                        terminal_pane_with_command(10, "cargo"),
                    ],
                ),
                (
                    1,
                    vec![
                        PaneInfo {
                            id: 21,
                            is_held: true,
                            ..terminal_pane_with_command(21, "less")
                        },
                        terminal_pane_with_command(20, "codex"),
                    ],
                ),
            ]
            .into_iter()
            .collect(),
        };

        assert2::assert!(update_panes(
            &mut state,
            &manifest,
            |pane_id| Some(PathBuf::from(format!("/tmp/pane-{pane_id}"))),
            |pane_id| Some(vec![format!("cmd-{pane_id}")]),
        ));

        let pane_ids = state.pane_entries.iter().map(|entry| entry.pane_id).collect::<Vec<_>>();
        assert_eq!(pane_ids, vec![10, 11, 20, 21]);
        assert_eq!(
            frame(&mut state)
                .iter()
                .map(|row| row.cwd_label.as_str())
                .collect::<Vec<_>>(),
            vec!["/tmp/pane-10", "/tmp/pane-11", "/tmp/pane-20", "/tmp/pane-21",]
        );
    }

    #[test]
    fn test_update_panes_selects_initial_focused_pane_from_manifest() {
        let mut state = PpickState::default();
        let manifest = PaneManifest {
            panes: std::iter::once((
                0,
                vec![
                    terminal_pane_with_command(42, "cargo"),
                    PaneInfo {
                        is_focused: true,
                        ..terminal_pane_with_command(43, "nvim")
                    },
                ],
            ))
            .collect(),
        };

        assert2::assert!(update_panes(&mut state, &manifest, |_| None, |_| None));

        assert_eq!(state.selected, 1);
        assert_eq!(frame(&mut state).get(1).map(|row| row.selected), Some(true));
    }

    #[test]
    fn test_update_panes_selects_active_tab_when_focus_is_not_observed() {
        let mut state = PpickState::default();
        let _ = state.update_tabs(vec![
            TabInfo {
                tab_id: 10,
                position: 0,
                ..Default::default()
            },
            TabInfo {
                tab_id: 20,
                position: 1,
                active: true,
                ..Default::default()
            },
        ]);
        let manifest = PaneManifest {
            panes: [
                (0, vec![terminal_pane_with_command(42, "cargo")]),
                (1, vec![terminal_pane_with_command(43, "nvim")]),
            ]
            .into_iter()
            .collect(),
        };

        assert2::assert!(update_panes(&mut state, &manifest, |_| None, |_| None));

        assert_eq!(state.selected, 1);
        assert_eq!(frame(&mut state).get(1).map(|row| row.selected), Some(true));
    }

    #[test]
    fn test_set_initial_focus_pane_selects_matching_pane_when_entries_arrive() {
        let mut state = PpickState::default();
        let _ = state.update_tabs(vec![TabInfo {
            tab_id: 10,
            position: 0,
            active: true,
            ..Default::default()
        }]);
        assert2::assert!(!state.set_initial_focus_pane(10, 43, 1));
        let manifest = PaneManifest {
            panes: std::iter::once((
                0,
                vec![
                    terminal_pane_with_command(42, "cargo"),
                    terminal_pane_with_command(43, "nvim"),
                ],
            ))
            .collect(),
        };

        assert2::assert!(update_panes(&mut state, &manifest, |_| None, |_| None));

        assert_eq!(state.selected, 1);
        assert_eq!(frame(&mut state).get(1).map(|row| row.selected), Some(true));
    }

    #[test]
    fn test_initial_focus_snapshot_ignores_older_snapshot_for_same_tab() {
        let mut state = PpickState::default();
        let _ = state.update_tabs(vec![TabInfo {
            tab_id: 10,
            position: 0,
            active: true,
            ..Default::default()
        }]);
        let manifest = PaneManifest {
            panes: std::iter::once((
                0,
                vec![
                    terminal_pane_with_command(42, "cargo"),
                    terminal_pane_with_command(43, "nvim"),
                ],
            ))
            .collect(),
        };
        let _ = update_panes(&mut state, &manifest, |_| None, |_| None);

        assert2::assert!(state.set_initial_focus_pane(10, 43, 2));
        assert2::assert!(!state.set_initial_focus_pane(10, 42, 1));

        assert_eq!(state.selected, 1);
        assert_eq!(frame(&mut state).get(1).map(|row| row.selected), Some(true));
    }

    #[test]
    fn test_initial_focus_snapshot_accepts_newer_snapshot_for_same_tab() {
        let mut state = PpickState::default();
        let _ = state.update_tabs(vec![TabInfo {
            tab_id: 10,
            position: 0,
            active: true,
            ..Default::default()
        }]);
        let manifest = PaneManifest {
            panes: std::iter::once((
                0,
                vec![
                    terminal_pane_with_command(42, "cargo"),
                    terminal_pane_with_command(43, "nvim"),
                ],
            ))
            .collect(),
        };
        let _ = update_panes(&mut state, &manifest, |_| None, |_| None);

        assert2::assert!(!state.set_initial_focus_pane(10, 42, 1));
        assert2::assert!(state.set_initial_focus_pane(10, 43, 2));

        assert_eq!(state.selected, 1);
        assert_eq!(frame(&mut state).get(1).map(|row| row.selected), Some(true));
    }

    #[test]
    fn test_initial_focus_selection_does_not_override_user_selection() {
        let mut state = PpickState::default();
        let _ = state.update_tabs(vec![TabInfo {
            tab_id: 10,
            position: 0,
            active: true,
            ..Default::default()
        }]);
        let _ = state.set_initial_focus_pane(10, 42, 1);
        let manifest = PaneManifest {
            panes: std::iter::once((
                0,
                vec![
                    terminal_pane_with_command(42, "cargo"),
                    terminal_pane_with_command(43, "nvim"),
                ],
            ))
            .collect(),
        };
        let _ = update_panes(&mut state, &manifest, |_| None, |_| None);
        let ctrl_n = KeyWithModifier::new_with_modifiers(BareKey::Char('n'), BTreeSet::from([KeyModifier::Ctrl]));

        assert_eq!(state.selected, 0);
        assert_eq!(state.handle_key(&ctrl_n), PpickAction::Redraw);
        assert_eq!(state.selected, 1);
        let _ = update_panes(&mut state, &manifest, |_| None, |_| None);

        assert_eq!(state.selected, 1);
    }

    #[test]
    fn test_initial_focus_snapshot_waits_for_active_tab_metadata() {
        let mut state = PpickState::default();
        assert2::assert!(!state.set_initial_focus_pane(20, 43, 1));
        let manifest = PaneManifest {
            panes: [
                (0, vec![terminal_pane_with_command(42, "cargo")]),
                (1, vec![terminal_pane_with_command(43, "nvim")]),
            ]
            .into_iter()
            .collect(),
        };

        assert2::assert!(update_panes(&mut state, &manifest, |_| None, |_| None));
        assert_eq!(state.selected, 0);
        assert2::assert!(state.update_tabs(vec![
            TabInfo {
                tab_id: 10,
                position: 0,
                ..Default::default()
            },
            TabInfo {
                tab_id: 20,
                position: 1,
                active: true,
                ..Default::default()
            },
        ]));

        assert_eq!(state.selected, 1);
        assert_eq!(frame(&mut state).get(1).map(|row| row.selected), Some(true));
    }

    #[test]
    fn test_initial_focus_snapshot_from_inactive_tab_does_not_select_pane() {
        let mut state = PpickState::default();
        let _ = state.update_tabs(vec![
            TabInfo {
                tab_id: 10,
                position: 0,
                active: true,
                ..Default::default()
            },
            TabInfo {
                tab_id: 20,
                position: 1,
                ..Default::default()
            },
        ]);
        let manifest = PaneManifest {
            panes: [
                (0, vec![terminal_pane_with_command(42, "cargo")]),
                (1, vec![terminal_pane_with_command(43, "nvim")]),
            ]
            .into_iter()
            .collect(),
        };
        let _ = update_panes(&mut state, &manifest, |_| None, |_| None);

        assert2::assert!(!state.set_initial_focus_pane(20, 43, 1));

        assert_eq!(state.selected, 0);
        assert_eq!(frame(&mut state).first().map(|row| row.selected), Some(true));
    }

    #[test]
    fn test_update_tabs_orders_panes_by_tab_order_then_pane_id() {
        let mut state = PpickState::default();
        let _ = state.update_tabs(vec![
            TabInfo {
                tab_id: 30,
                position: 2,
                ..Default::default()
            },
            TabInfo {
                tab_id: 10,
                position: 0,
                ..Default::default()
            },
        ]);
        state.pane_entries = vec![
            PaneEntry::new(2, 30, None, vec![String::from("third")], None),
            PaneEntry::new(0, 11, None, vec![String::from("first-b")], None),
            PaneEntry::new(0, 10, None, vec![String::from("first-a")], None),
        ];

        assert2::assert!(state.update_tabs(state.all_tabs.clone()));

        let pane_ids = state.pane_entries.iter().map(|entry| entry.pane_id).collect::<Vec<_>>();
        assert_eq!(pane_ids, vec![10, 11, 30]);
    }

    #[test]
    fn test_update_tabs_keeps_selected_pane_after_tab_order_changes() {
        let mut state = PpickState {
            pane_entries: vec![
                PaneEntry::new(0, 42, None, vec![String::from("cargo")], None),
                PaneEntry::new(1, 43, None, vec![String::from("nvim")], None),
            ],
            ..Default::default()
        };
        let _ = state.update_tabs(vec![
            TabInfo {
                tab_id: 10,
                position: 0,
                ..Default::default()
            },
            TabInfo {
                tab_id: 20,
                position: 1,
                ..Default::default()
            },
        ]);
        assert_eq!(state.handle_key(&key(BareKey::Down)), PpickAction::Redraw);

        assert2::assert!(state.update_tabs(vec![
            TabInfo {
                tab_id: 20,
                position: 0,
                ..Default::default()
            },
            TabInfo {
                tab_id: 10,
                position: 1,
                ..Default::default()
            },
        ]));

        assert_eq!(state.selected_pane_id, Some(43));
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn test_update_panes_keeps_stable_tab_id_when_panes_refresh_before_tabs_after_tab_move() {
        let mut state = PpickState::default();
        let _ = state.update_tabs(vec![
            TabInfo {
                tab_id: 10,
                position: 0,
                ..Default::default()
            },
            TabInfo {
                tab_id: 20,
                position: 1,
                ..Default::default()
            },
        ]);
        let manifest = PaneManifest {
            panes: std::iter::once((1, vec![terminal_pane_with_command(43, "nvim")])).collect(),
        };
        let _ = update_panes(&mut state, &manifest, |_| None, |_| None);
        let rows = frame(&mut state);
        assert_eq!(rows.first().map(|row| row.pane_label.as_str()), Some("20:43"));

        let stale_tabs_manifest = PaneManifest {
            panes: std::iter::once((0, vec![terminal_pane_with_command(43, "nvim")])).collect(),
        };
        let _ = update_panes(&mut state, &stale_tabs_manifest, |_| None, |_| None);
        let rows = frame(&mut state);
        assert_eq!(rows.first().map(|row| row.pane_label.as_str()), Some("20:43"));

        let _ = state.update_tabs(vec![
            TabInfo {
                tab_id: 20,
                position: 0,
                ..Default::default()
            },
            TabInfo {
                tab_id: 10,
                position: 1,
                ..Default::default()
            },
        ]);

        let rows = frame(&mut state);
        assert_eq!(rows.first().map(|row| row.pane_label.as_str()), Some("20:43"));
    }

    #[test]
    fn test_visible_frame_shows_compact_pane_label_for_each_entry() {
        let mut state = PpickState::default();
        let _ = state.update_tabs(vec![
            TabInfo {
                tab_id: 10,
                position: 0,
                ..Default::default()
            },
            TabInfo {
                tab_id: 20,
                position: 1,
                ..Default::default()
            },
        ]);
        let manifest = PaneManifest {
            panes: [
                (
                    0,
                    vec![
                        terminal_pane_with_command(42, "cargo"),
                        terminal_pane_with_command(43, "nvim"),
                    ],
                ),
                (1, vec![terminal_pane_with_command(44, "git")]),
            ]
            .into_iter()
            .collect(),
        };
        let _ = update_panes(&mut state, &manifest, |_| None, |_| None);

        let rows = frame(&mut state);
        let labels = rows.iter().map(|row| row.pane_label.as_str()).collect::<Vec<_>>();

        assert_eq!(labels, vec!["10:42", "10:43", "20:44"]);
    }

    #[test]
    fn test_agent_only_mode_filters_non_agent_panes_and_keeps_seen_busy_and_unseen_agents() {
        let mut busy = PaneEntry::new(0, 44, None, vec![String::from("claude")], None);
        let _ = busy.apply_agent_snapshot(PaneAgentSnapshot {
            pane_id: 44,
            agent: Agent::Claude,
            indicator: agg::TabIndicator::Busy,
        });
        let mut unseen = PaneEntry::new(0, 45, None, vec![String::from("cursor-agent")], None);
        let _ = unseen.apply_agent_snapshot(PaneAgentSnapshot {
            pane_id: 45,
            agent: Agent::Cursor,
            indicator: agg::TabIndicator::Unseen,
        });
        let mut state = PpickState {
            pane_entries: vec![
                PaneEntry::new(0, 42, None, vec![String::from("cargo")], None),
                PaneEntry::new(0, 43, None, vec![String::from("codex")], None),
                busy,
                unseen,
            ],
            ..PpickState::new(PpickMode::AgentsOnly)
        };

        let rows = frame(&mut state);
        let labels = rows.iter().map(|row| row.pane_label.as_str()).collect::<Vec<_>>();
        let indicators = rows.iter().map(|row| row.indicator).collect::<Vec<_>>();

        assert_eq!(labels, vec!["43", "44", "45"]);
        assert_eq!(
            indicators,
            vec![
                agg::TabIndicator::Seen,
                agg::TabIndicator::Busy,
                agg::TabIndicator::Unseen,
            ]
        );
    }

    #[test]
    fn test_agent_only_mode_closes_empty_picker_only_without_query() {
        let mut state = PpickState {
            pane_entries: vec![PaneEntry::new(0, 42, None, vec![String::from("cargo")], None)],
            ..PpickState::new(PpickMode::AgentsOnly)
        };

        assert2::assert!(state.should_close_empty_picker());

        state.query = String::from("codex");
        state.mark_filter_dirty();
        assert2::assert!(!state.should_close_empty_picker());
    }

    #[test]
    fn test_all_panes_mode_does_not_close_empty_picker() {
        let mut state = PpickState {
            pane_entries: vec![PaneEntry::new(0, 42, None, vec![String::from("cargo")], None)],
            ..Default::default()
        };

        assert2::assert!(!state.should_close_empty_picker());
    }

    #[test]
    fn test_search_matches_pane_path_command_and_agent_label() {
        let entry = PaneEntry::new(
            0,
            42,
            Some(PathBuf::from("/Users/me/project")),
            vec![String::from("codex"), String::from("resume"), String::from("abc")],
            None,
        );

        for query in ["42", "project", "codex", "cx"] {
            assert2::assert!(entry.matches_normalized_query(query));
        }
    }

    #[test]
    fn test_handle_key_selection_uses_cached_filter_for_large_result() {
        let mut state = PpickState {
            pane_entries: (0..1_000)
                .map(|idx| {
                    PaneEntry::new(
                        0,
                        idx,
                        Some(PathBuf::from(format!("/tmp/work-{idx}"))),
                        vec![String::from("cargo")],
                        None,
                    )
                })
                .collect(),
            ..Default::default()
        };
        for c in "work".chars() {
            assert_eq!(state.handle_key(&key(BareKey::Char(c))), PpickAction::Redraw);
        }
        assert_eq!(state.filtered_entry_indices.len(), 1_000);
        let filtered_entry_indices = state.filtered_entry_indices.clone();

        assert_eq!(state.handle_key(&key(BareKey::Down)), PpickAction::Redraw);

        assert_eq!(state.selected, 1);
        assert_eq!(state.filtered_entry_indices, filtered_entry_indices);
        assert_eq!(state.handle_key(&key(BareKey::Enter)), PpickAction::Focus(1));
    }

    #[test]
    fn test_visible_frame_materializes_only_capacity_rows_and_keeps_selection_visible() {
        let mut state = PpickState {
            pane_entries: (0..8)
                .map(|idx| {
                    PaneEntry::new(
                        0,
                        idx,
                        Some(PathBuf::from(format!("/tmp/pane-{idx}"))),
                        vec![String::from("cargo")],
                        None,
                    )
                })
                .collect(),
            ..Default::default()
        };
        for _ in 0..5 {
            assert_eq!(state.handle_key(&key(BareKey::Down)), PpickAction::Redraw);
        }

        let frame = state.visible_frame(2);

        assert_eq!(frame.len(), 2);
        assert_eq!(
            frame.iter().map(|row| row.cwd_label.as_str()).collect::<Vec<_>>(),
            vec!["/tmp/pane-4", "/tmp/pane-5"]
        );
        assert_eq!(
            frame.iter().map(|row| row.selected).collect::<Vec<_>>(),
            vec![false, true]
        );
    }

    #[test]
    fn test_pane_update_resolves_running_command_only_for_uncached_panes() {
        let mut state = PpickState::default();
        let manifest = PaneManifest {
            panes: std::iter::once((
                0,
                vec![
                    terminal_pane_with_command(42, "zsh"),
                    terminal_pane_with_command(43, "zsh"),
                ],
            ))
            .collect(),
        };
        let command_calls = Cell::new(0);

        let _ = update_panes(
            &mut state,
            &manifest,
            |pane_id| Some(PathBuf::from(format!("/tmp/pane-{pane_id}"))),
            |pane_id| {
                command_calls.set(command_calls.get() + 1);
                Some(vec![format!("cmd-{pane_id}")])
            },
        );
        let _ = update_panes(
            &mut state,
            &manifest,
            |pane_id| Some(PathBuf::from(format!("/tmp/changed-{pane_id}"))),
            |pane_id| {
                command_calls.set(command_calls.get() + 1);
                Some(vec![format!("changed-{pane_id}")])
            },
        );

        assert_eq!(command_calls.get(), 2);
        assert_eq!(
            state
                .pane_entries
                .iter()
                .map(|entry| entry.command_args.clone())
                .collect::<Vec<_>>(),
            vec![vec![String::from("cmd-42")], vec![String::from("cmd-43")]]
        );
    }

    #[test]
    fn test_git_stat_refreshes_first_seen_and_cwd_changes_not_command_changes() {
        let mut state = PpickState::default();
        let manifest = PaneManifest {
            panes: std::iter::once((0, vec![terminal_pane_with_command(42, "zsh")])).collect(),
        };

        let _ = update_panes(
            &mut state,
            &manifest,
            |_| Some(PathBuf::from("/tmp/repo")),
            |_| Some(vec![String::from("cargo")]),
        );
        assert_eq!(state.take_git_stat_cwds_to_request(), vec![PathBuf::from("/tmp/repo")]);

        assert2::assert!(state.update_command(42, &[String::from("nvim")]));
        assert_eq!(state.take_git_stat_cwds_to_request(), Vec::<PathBuf>::new());

        assert2::assert!(state.update_cwd(42, &PathBuf::from("/tmp/other")));
        assert_eq!(state.take_git_stat_cwds_to_request(), vec![PathBuf::from("/tmp/other")]);
    }

    #[test]
    fn test_git_stat_requests_dedupe_while_in_flight() {
        let mut state = PpickState::default();
        let manifest = PaneManifest {
            panes: std::iter::once((0, vec![terminal_pane_with_command(42, "zsh")])).collect(),
        };
        let repo = PathBuf::from("/tmp/repo");
        let other = PathBuf::from("/tmp/other");
        let _ = update_panes(
            &mut state,
            &manifest,
            |_| Some(repo.clone()),
            |_| Some(vec![String::from("cargo")]),
        );

        assert_eq!(state.take_git_stat_cwds_to_request(), vec![repo.clone()]);
        assert2::assert!(state.update_cwd(42, &other));
        assert_eq!(state.take_git_stat_cwds_to_request(), vec![other.clone()]);
        assert2::assert!(state.update_cwd(42, &repo));
        assert_eq!(state.take_git_stat_cwds_to_request(), Vec::<PathBuf>::new());

        state.finish_git_stat_request(&repo);
        assert2::assert!(state.update_cwd(42, &other));
        assert_eq!(state.take_git_stat_cwds_to_request(), Vec::<PathBuf>::new());
        state.finish_git_stat_request(&other);
        assert2::assert!(state.update_cwd(42, &repo));
        assert_eq!(state.take_git_stat_cwds_to_request(), vec![repo]);
    }

    #[test]
    fn test_search_matches_attached_ags_hidden_session_text() {
        let mut state = PpickState {
            pane_entries: vec![PaneEntry::new(
                0,
                42,
                Some(PathBuf::from("/tmp/repo")),
                vec![String::from("codex"), String::from("resume"), String::from("older")],
                None,
            )],
            ..Default::default()
        };

        assert2::assert!(state.update_sessions(vec![session_entry(
            "codex",
            "/tmp/repo",
            "older",
            "hidden prompt about billing",
            "2026-05-09T09:00:00Z",
        )]));

        state.query = String::from("BILLING");
        let frame = frame(&mut state);
        assert_eq!(frame.len(), 1);
    }

    #[test]
    fn test_frame_carries_attached_ags_session_summary() {
        let mut state = PpickState {
            pane_entries: vec![PaneEntry::new(
                0,
                42,
                Some(PathBuf::from("/tmp/repo")),
                vec![
                    String::from("codex"),
                    String::from("resume"),
                    String::from("session-id"),
                ],
                None,
            )],
            ..Default::default()
        };
        let mut session = session_entry(
            "codex",
            "/tmp/repo",
            "session-id",
            "hidden prompt",
            "2026-05-09T09:00:00Z",
        );
        session.summary = Some(String::from("how to solve this warning"));

        let _ = state.update_sessions(vec![session]);

        let frame = frame(&mut state);
        assert_eq!(
            frame.first().map(|row| row.session_summary.as_str()),
            Some("how to solve this warning")
        );
    }

    #[test]
    fn test_git_stat_update_updates_matching_pane_frame() {
        let mut state = PpickState {
            pane_entries: vec![
                PaneEntry::new(
                    0,
                    42,
                    Some(PathBuf::from("/tmp/repo")),
                    vec![String::from("cargo")],
                    None,
                ),
                PaneEntry::new(
                    0,
                    43,
                    Some(PathBuf::from("/tmp/other")),
                    vec![String::from("nvim")],
                    None,
                ),
            ],
            ..Default::default()
        };
        let stat = agg::GitStat {
            path: PathBuf::from("/tmp/repo"),
            branch: Some("main".to_string()),
            insertions: 2,
            deletions: 1,
            new_files: 3,
            is_worktree: false,
            ..Default::default()
        };

        assert2::assert!(state.update_git_stat(&stat));

        let frame = frame(&mut state);
        assert_eq!(frame.first().map(|row| &row.git), Some(&stat));
        assert_eq!(frame.first().map(|row| row.branch_label.as_str()), Some("main"));
        assert_eq!(frame.get(1).map(|row| &row.git), Some(&agg::GitStat::default()));
    }

    #[test]
    fn test_agent_events_follow_tbar_marker_transitions() {
        let mut state = PpickState::default();
        let _ = state.update_tabs(vec![TabInfo {
            active: true,
            position: 0,
            ..Default::default()
        }]);
        let manifest = PaneManifest {
            panes: std::iter::once((0, vec![terminal_pane_with_command(42, "codex")])).collect(),
        };
        let _ = update_panes(&mut state, &manifest, |_| None, |_| Some(vec![String::from("codex")]));

        assert2::assert!(state.update_agent(&AgentEventPayload {
            pane_id: 42,
            agent: Agent::Codex,
            kind: AgentEventKind::Busy,
        }));
        assert_eq!(
            frame(&mut state).first().map(|row| row.indicator),
            Some(agg::TabIndicator::Busy)
        );

        assert2::assert!(state.update_agent(&AgentEventPayload {
            pane_id: 42,
            agent: Agent::Codex,
            kind: AgentEventKind::Idle,
        }));
        assert_eq!(
            frame(&mut state).first().map(|row| row.indicator),
            Some(agg::TabIndicator::Unseen)
        );

        let focused_manifest = PaneManifest {
            panes: std::iter::once((
                0,
                vec![PaneInfo {
                    is_focused: true,
                    ..terminal_pane_with_command(42, "codex")
                }],
            ))
            .collect(),
        };
        let _ = update_panes(
            &mut state,
            &focused_manifest,
            |_| None,
            |_| Some(vec![String::from("codex")]),
        );

        assert_eq!(
            frame(&mut state).first().map(|row| row.indicator),
            Some(agg::TabIndicator::Seen)
        );
    }

    #[test]
    fn test_state_snapshot_before_pane_update_hydrates_busy_agent() {
        let mut state = PpickState::default();
        let snapshot = StateSnapshotPayload {
            tab_id: 10,
            seq: 1,
            focused_pane_id: Some(42),
            cwd: None,
            cmd: agg::Cmd::None,
            indicator: agg::TabIndicator::NoAgent,
            git_stat: agg::GitStat::default(),
            pane_agents: vec![PaneAgentSnapshot {
                pane_id: 42,
                agent: Agent::Codex,
                indicator: agg::TabIndicator::Busy,
            }],
        };
        let _ = state.apply_state_snapshot(&snapshot);
        let _ = state.update_tabs(vec![TabInfo {
            tab_id: 10,
            active: true,
            position: 0,
            ..Default::default()
        }]);
        let manifest = PaneManifest {
            panes: std::iter::once((0, vec![terminal_pane_with_command(42, "codex")])).collect(),
        };

        let _ = update_panes(&mut state, &manifest, |_| None, |_| Some(vec![String::from("codex")]));

        assert_eq!(
            frame(&mut state).first().map(|row| row.indicator),
            Some(agg::TabIndicator::Busy)
        );
    }

    #[test]
    fn test_state_snapshot_after_pane_update_replaces_seen_with_unseen() {
        let mut state = PpickState::default();
        let _ = state.update_tabs(vec![TabInfo {
            tab_id: 10,
            active: true,
            position: 0,
            ..Default::default()
        }]);
        let manifest = PaneManifest {
            panes: std::iter::once((0, vec![terminal_pane_with_command(42, "codex")])).collect(),
        };
        let _ = update_panes(&mut state, &manifest, |_| None, |_| Some(vec![String::from("codex")]));
        assert_eq!(
            frame(&mut state).first().map(|row| row.indicator),
            Some(agg::TabIndicator::Seen)
        );

        let snapshot = StateSnapshotPayload {
            tab_id: 10,
            seq: 1,
            focused_pane_id: Some(42),
            cwd: None,
            cmd: agg::Cmd::None,
            indicator: agg::TabIndicator::NoAgent,
            git_stat: agg::GitStat::default(),
            pane_agents: vec![PaneAgentSnapshot {
                pane_id: 42,
                agent: Agent::Codex,
                indicator: agg::TabIndicator::Unseen,
            }],
        };

        assert2::assert!(state.apply_state_snapshot(&snapshot));

        assert_eq!(
            frame(&mut state).first().map(|row| row.indicator),
            Some(agg::TabIndicator::Unseen)
        );
    }

    #[test]
    fn test_agent_event_invalidates_state_snapshot_cache() {
        let mut state = PpickState::default();
        let snapshot = StateSnapshotPayload {
            tab_id: 10,
            seq: 1,
            focused_pane_id: Some(42),
            cwd: None,
            cmd: agg::Cmd::None,
            indicator: agg::TabIndicator::NoAgent,
            git_stat: agg::GitStat::default(),
            pane_agents: vec![PaneAgentSnapshot {
                pane_id: 42,
                agent: Agent::Codex,
                indicator: agg::TabIndicator::Busy,
            }],
        };
        let _ = state.apply_state_snapshot(&snapshot);
        let _ = state.update_tabs(vec![TabInfo {
            tab_id: 10,
            active: true,
            position: 0,
            ..Default::default()
        }]);
        let manifest = PaneManifest {
            panes: std::iter::once((0, vec![terminal_pane_with_command(42, "codex")])).collect(),
        };
        let _ = update_panes(&mut state, &manifest, |_| None, |_| Some(vec![String::from("codex")]));

        assert_eq!(
            frame(&mut state).first().map(|row| row.indicator),
            Some(agg::TabIndicator::Busy)
        );

        assert2::assert!(state.update_agent(&AgentEventPayload {
            pane_id: 42,
            agent: Agent::Codex,
            kind: AgentEventKind::Idle,
        }));

        let focused_manifest = PaneManifest {
            panes: std::iter::once((
                0,
                vec![PaneInfo {
                    is_focused: true,
                    ..terminal_pane_with_command(42, "codex")
                }],
            ))
            .collect(),
        };
        let _ = update_panes(
            &mut state,
            &focused_manifest,
            |_| None,
            |_| Some(vec![String::from("codex")]),
        );

        assert_eq!(
            frame(&mut state).first().map(|row| row.indicator),
            Some(agg::TabIndicator::Seen)
        );
    }

    #[test]
    fn test_agent_pane_without_session_id_does_not_attach_ags_data() {
        let mut state = PpickState {
            pane_entries: vec![PaneEntry::new(
                0,
                42,
                Some(PathBuf::from("/tmp/repo")),
                vec![String::from("codex")],
                None,
            )],
            query: String::from("repo"),
            ..Default::default()
        };

        assert2::assert!(state.update_sessions(vec![session_entry(
            "codex",
            "/tmp/repo",
            "session-id",
            "hidden",
            "2026-05-09T09:00:00Z",
        )]));

        assert_eq!(frame(&mut state).len(), 1);
        assert_eq!(
            state
                .pane_entries
                .first()
                .and_then(|entry| entry.session_search.as_deref()),
            None
        );
    }

    #[test]
    fn test_exact_session_id_match_attaches_ags_data() {
        let mut state = PpickState {
            pane_entries: vec![PaneEntry::new(
                0,
                42,
                Some(PathBuf::from("/tmp/repo")),
                vec![String::from("codex"), String::from("resume"), String::from("exact")],
                None,
            )],
            ..Default::default()
        };

        let _ = state.update_sessions(vec![
            session_entry("codex", "/tmp/repo", "new", "new hidden", "2026-05-09T10:00:00Z"),
            session_entry("codex", "/tmp/repo", "exact", "exact hidden", "2026-05-09T09:00:00Z"),
        ]);

        assert_eq!(
            state
                .pane_entries
                .first()
                .and_then(|entry| entry.session_search.as_deref()),
            Some("exact hidden")
        );
    }

    #[test]
    fn test_handle_key_updates_query_backspace_esc_empty_and_enter() {
        let mut state = PpickState {
            pane_entries: vec![PaneEntry::new(0, 42, None, vec![String::from("cargo")], None)],
            ..Default::default()
        };

        assert_eq!(state.handle_key(&key(BareKey::Char('c'))), PpickAction::Redraw);
        assert_eq!(state.handle_key(&key(BareKey::Backspace)), PpickAction::Redraw);
        assert_eq!(state.query, "");
        assert_eq!(state.handle_key(&key(BareKey::Esc)), PpickAction::Close);
        assert_eq!(state.handle_key(&key(BareKey::Enter)), PpickAction::Focus(42));
        assert_eq!(state.handle_key(&key(BareKey::Char('x'))), PpickAction::Redraw);
        assert_eq!(state.handle_key(&key(BareKey::Enter)), PpickAction::None);
    }

    #[test]
    fn test_handle_key_ctrl_n_and_ctrl_p_loop_selection() {
        let mut state = PpickState {
            pane_entries: vec![
                PaneEntry::new(0, 42, None, vec![String::from("cargo")], None),
                PaneEntry::new(0, 43, None, vec![String::from("nvim")], None),
            ],
            ..Default::default()
        };
        let ctrl_n = KeyWithModifier::new_with_modifiers(BareKey::Char('n'), BTreeSet::from([KeyModifier::Ctrl]));
        let ctrl_p = KeyWithModifier::new_with_modifiers(BareKey::Char('p'), BTreeSet::from([KeyModifier::Ctrl]));

        assert_eq!(state.handle_key(&ctrl_n), PpickAction::Redraw);
        assert_eq!(state.selected, 1);
        assert_eq!(state.handle_key(&ctrl_n), PpickAction::Redraw);
        assert_eq!(state.selected, 0);
        assert_eq!(state.handle_key(&ctrl_p), PpickAction::Redraw);
        assert_eq!(state.selected, 1);
        assert_eq!(state.handle_key(&ctrl_p), PpickAction::Redraw);
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn test_handle_key_selection_loop_is_noop_for_empty_or_single_result() {
        let ctrl_n = KeyWithModifier::new_with_modifiers(BareKey::Char('n'), BTreeSet::from([KeyModifier::Ctrl]));
        let ctrl_p = KeyWithModifier::new_with_modifiers(BareKey::Char('p'), BTreeSet::from([KeyModifier::Ctrl]));
        let mut empty_state = PpickState::default();
        let mut single_state = PpickState {
            pane_entries: vec![PaneEntry::new(0, 42, None, vec![String::from("cargo")], None)],
            ..Default::default()
        };

        assert_eq!(empty_state.handle_key(&ctrl_n), PpickAction::None);
        assert_eq!(empty_state.handle_key(&ctrl_p), PpickAction::None);
        assert_eq!(single_state.handle_key(&ctrl_n), PpickAction::None);
        assert_eq!(single_state.handle_key(&ctrl_p), PpickAction::None);
        assert_eq!(single_state.selected, 0);
    }
}
