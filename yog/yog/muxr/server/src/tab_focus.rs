use rootcause::report;

use crate::state::SessionLayout;

impl SessionLayout {
    pub fn focus_previous_tab(&mut self) -> rootcause::Result<()> {
        let tab_index = self.active_tab_index()?;
        let previous_index = if tab_index == 0 {
            self.entries.len().saturating_sub(1)
        } else {
            tab_index.saturating_sub(1)
        };
        self.active_tab = self
            .entries
            .get(previous_index)
            .ok_or_else(|| report!("muxr previous tab is missing from server layout"))?
            .id
            .clone();
        Ok(())
    }

    pub fn focus_next_tab(&mut self) -> rootcause::Result<()> {
        let tab_index = self.active_tab_index()?;
        let next_index = tab_index
            .checked_add(1)
            .filter(|index| *index < self.entries.len())
            .unwrap_or(0);
        self.active_tab = self
            .entries
            .get(next_index)
            .ok_or_else(|| report!("muxr next tab is missing from server layout"))?
            .id
            .clone();
        Ok(())
    }
}

pub fn handle_focus_previous_tab(layout: &mut SessionLayout) -> rootcause::Result<()> {
    layout.focus_previous_tab()
}

pub fn handle_focus_next_tab(layout: &mut SessionLayout) -> rootcause::Result<()> {
    layout.focus_next_tab()
}
