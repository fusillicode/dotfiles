use muxr_core::PaneId;
use muxr_core::TabId;
use muxr_core::TerminalSize;
use rootcause::report;

use crate::client::session::ClientSessionState;
use crate::pane::runtime::PaneRuntimes;
use crate::server::ServerConfig;
use crate::state::Pane;
use crate::state::PaneAttentionState;
use crate::state::PaneState;
use crate::state::PaneTree;
use crate::state::SessionLayout;
use crate::state::SessionMetadata;
use crate::state::Tab;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TabCreateClientOutcome {
    pub new_pane_id: PaneId,
    pub previous_pane: PaneId,
}

impl SessionLayout {
    pub fn create_tab(&mut self, metadata: SessionMetadata) -> rootcause::Result<PaneId> {
        let tab_index = self.active_tab_index()?;
        let tab_id = TabId::new(self.next_tab_number()?)?;
        let pane_id = PaneId::new(self.next_pane_number()?)?;
        let insert_index = tab_index
            .checked_add(1)
            .ok_or_else(|| report!("muxr tab insert index overflowed"))?;

        self.entries.insert(
            insert_index,
            Tab {
                active_pane: pane_id,
                id: tab_id,
                pane_tree: PaneTree::Pane(Pane {
                    attention_state: PaneAttentionState::Idle,
                    cmd_label: metadata.cmd_label.clone(),
                    cwd: metadata.cwd,
                    focus_seq: 1,
                    id: pane_id,
                    started_at: metadata.started_at,
                    state: PaneState::Running,
                    title: metadata.cmd_label,
                }),
                title: format!("tab {}", tab_id.get()),
            },
        );
        self.active_tab = tab_id;
        Ok(pane_id)
    }
}

fn handle_create_tab_cmd(
    config: &ServerConfig,
    layout: &mut SessionLayout,
    runtimes: &mut PaneRuntimes,
    terminal_size: &TerminalSize,
) -> rootcause::Result<PaneId> {
    let pane_id = self::handle_create_tab(layout, config, runtimes, terminal_size)?;
    crate::state::persisted::write_metadata(&config.paths, layout)?;
    Ok(pane_id)
}

pub fn handle_create_tab_cmd_client(state: &mut ClientSessionState<'_>) -> rootcause::Result<TabCreateClientOutcome> {
    let previous_pane = state.layout.active_pane_id()?;
    let new_pane_id = self::handle_create_tab_cmd(state.config, state.layout, state.runtimes, &state.terminal_size)?;
    Ok(TabCreateClientOutcome {
        new_pane_id,
        previous_pane,
    })
}

fn handle_create_tab(
    layout: &mut SessionLayout,
    config: &ServerConfig,
    runtimes: &mut PaneRuntimes,
    terminal_size: &TerminalSize,
) -> rootcause::Result<PaneId> {
    crate::pane::runtime::sync_layout_terminal_titles(layout, runtimes)?;
    let metadata = crate::server::active_pane_session_metadata(config, layout)?;
    let previous_layout = layout.clone();
    let pane_id = layout.create_tab(metadata)?;
    crate::pane::runtime::spawn_pane_or_restore_layout(
        layout,
        previous_layout,
        pane_id,
        config,
        runtimes,
        terminal_size,
    )
}

#[cfg(test)]
mod tests {
    use muxr_core::TerminalSize;

    use super::*;
    use crate::pane::runtime::test_helpers as pane_runtime_test_helpers;
    use crate::server::test_helpers as server_test_helpers;
    use crate::state::test_helpers as state_test_helpers;

    #[test]
    fn test_handle_create_tab_when_pane_spawn_fails_restores_layout() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let mut config = server_test_helpers::server_config(tempdir.path(), "work")?;
        config.shell_cmd = server_test_helpers::shell_cmd("/bin/muxr-missing-shell");
        let initial_layout = SessionLayout::initial(&config.session, state_test_helpers::metadata("sh", 1))?;
        let mut layout = initial_layout.clone();
        let mut runtimes = pane_runtime_test_helpers::empty_runtimes();

        let create_result = self::handle_create_tab(&mut layout, &config, &mut runtimes, &TerminalSize::new(80, 24)?);
        assert2::assert!(create_result.is_err());

        pretty_assertions::assert_eq!(layout, initial_layout);
        pretty_assertions::assert_eq!(runtimes.set_status(), crate::pane::runtime::PaneRuntimeSetStatus::Empty);
        assert2::assert!(!config.paths.layout.exists());
        Ok(())
    }
}
