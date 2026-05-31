use rootcause::report;

use crate::state::SessionLayout;

impl SessionLayout {
    pub fn move_active_tab_previous(&mut self) -> rootcause::Result<()> {
        let tab_index = self.active_tab_index()?;
        if tab_index > 0 {
            self.entries.swap(tab_index, tab_index.saturating_sub(1));
        }
        Ok(())
    }

    pub fn move_active_tab_next(&mut self) -> rootcause::Result<()> {
        let tab_index = self.active_tab_index()?;
        let Some(next_index) = tab_index.checked_add(1) else {
            return Err(report!("muxr next tab index overflowed"));
        };
        if next_index < self.entries.len() {
            self.entries.swap(tab_index, next_index);
        }
        Ok(())
    }
}

pub fn handle_move_active_tab_previous(layout: &mut SessionLayout) -> rootcause::Result<()> {
    layout.move_active_tab_previous()
}

pub fn handle_move_active_tab_next(layout: &mut SessionLayout) -> rootcause::Result<()> {
    layout.move_active_tab_next()
}
