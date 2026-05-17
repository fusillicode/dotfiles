use crate::plugin::ppick::entry::PaneEntry;
use crate::plugin::ppick::state::InitialFocus;
use crate::plugin::ppick::state::PpickAction;
use crate::plugin::ppick::state::PpickMode;
use crate::plugin::ppick::state::PpickState;
use crate::plugin::ppick::ui::PpickRow;

impl PpickState {
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

    pub(super) fn select_next(&mut self) -> PpickAction {
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

    pub(super) fn select_previous(&mut self) -> PpickAction {
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

    pub(super) fn select_initial_focus(&mut self, observed_focused_pane_id: Option<u32>) -> bool {
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

    pub(super) fn clamp_selection(&mut self) -> bool {
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

    pub(super) const fn mark_filter_dirty(&mut self) {
        self.filter_ready = false;
    }

    pub(super) fn ensure_filter(&mut self) -> bool {
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

    pub(super) fn selected_entry(&self) -> Option<&PaneEntry> {
        self.filtered_entry_indices
            .get(self.selected)
            .and_then(|entry_idx| self.pane_entries.get(*entry_idx))
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

    fn set_selected(&mut self, selected: usize) {
        self.selected = selected;
        self.refresh_selected_pane_id();
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
