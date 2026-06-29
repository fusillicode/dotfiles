use rootcause::report;

use crate::client::session::ClientSessionState;
use crate::server::ServerConfig;
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

fn handle_move_active_tab_previous_cmd(config: &ServerConfig, layout: &mut SessionLayout) -> rootcause::Result<()> {
    layout.move_active_tab_previous()?;
    crate::state::persisted::write_metadata(&config.paths, layout)?;
    Ok(())
}

fn handle_move_active_tab_next_cmd(config: &ServerConfig, layout: &mut SessionLayout) -> rootcause::Result<()> {
    layout.move_active_tab_next()?;
    crate::state::persisted::write_metadata(&config.paths, layout)?;
    Ok(())
}

pub fn handle_move_active_tab_previous_cmd_client(state: &mut ClientSessionState<'_>) -> rootcause::Result<()> {
    self::handle_move_active_tab_previous_cmd(state.config, state.layout)
}

pub fn handle_move_active_tab_next_cmd_client(state: &mut ClientSessionState<'_>) -> rootcause::Result<()> {
    self::handle_move_active_tab_next_cmd(state.config, state.layout)
}

#[cfg(test)]
mod tests {
    use test_that::prelude::*;

    use crate::state::test_helpers as state_test_helpers;

    #[test]
    fn test_layout_tab_cmds_when_tabs_exist_mutates_active_tab_and_order() -> rootcause::Result<()> {
        let mut layout = state_test_helpers::layout("work")?;

        layout.create_tab(state_test_helpers::metadata("sh", 2))?;
        layout.create_tab(state_test_helpers::metadata("sh", 3))?;
        assert_that!(
            state_test_helpers::layout_tab_ids(&layout)?,
            eq(vec!["tab-1", "tab-2", "tab-3"])
        );
        assert_that!(layout.active_tab.to_string(), eq("tab-3"));

        layout.focus_previous_tab()?;
        assert_that!(layout.active_tab.to_string(), eq("tab-2"));
        layout.move_active_tab_previous()?;
        assert_that!(
            state_test_helpers::layout_tab_ids(&layout)?,
            eq(vec!["tab-2", "tab-1", "tab-3"])
        );
        assert_that!(layout.active_tab.to_string(), eq("tab-2"));
        layout.move_active_tab_next()?;
        assert_that!(
            state_test_helpers::layout_tab_ids(&layout)?,
            eq(vec!["tab-1", "tab-2", "tab-3"])
        );
        layout.focus_next_tab()?;
        assert_that!(layout.active_tab.to_string(), eq("tab-3"));
        Ok(())
    }
}
