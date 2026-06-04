use std::sync::Mutex;

use muxr_core::PaneId;
use muxr_core::TabId;
use muxr_core::TerminalSize;
use rootcause::report;

use crate::server::PaneRuntimes;
use crate::server::ServerConfig;
use crate::state::Pane;
use crate::state::PaneAttentionState;
use crate::state::PaneState;
use crate::state::PaneTree;
use crate::state::SessionLayout;
use crate::state::SessionMetadata;
use crate::state::Tab;

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

pub fn handle_create_tab(
    layout: &mut SessionLayout,
    config: &ServerConfig,
    runtimes: &Mutex<PaneRuntimes>,
    terminal_size: &TerminalSize,
) -> rootcause::Result<PaneId> {
    crate::server::sync_layout_terminal_titles(layout, runtimes)?;
    let metadata = crate::server::active_pane_session_metadata(config, layout)?;
    let previous_layout = layout.clone();
    let pane_id = layout.create_tab(metadata)?;
    crate::server::spawn_pane_or_restore_layout(layout, previous_layout, pane_id, config, runtimes, terminal_size)
}
