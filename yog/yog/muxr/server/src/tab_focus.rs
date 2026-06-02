use muxr_core::TabId;
use rootcause::report;

use crate::state::SessionLayout;

impl SessionLayout {
    pub fn focus_tab(&mut self, tab_id: &TabId) -> bool {
        if self.active_tab == *tab_id {
            return false;
        }
        if !self.entries.iter().any(|tab| tab.id == *tab_id) {
            return false;
        }
        self.active_tab = tab_id.clone();
        true
    }

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

pub fn handle_focus_tab(layout: &mut SessionLayout, tab_id: &TabId) -> bool {
    layout.focus_tab(tab_id)
}

pub fn handle_focus_previous_tab(layout: &mut SessionLayout) -> rootcause::Result<()> {
    layout.focus_previous_tab()
}

pub fn handle_focus_next_tab(layout: &mut SessionLayout) -> rootcause::Result<()> {
    layout.focus_next_tab()
}

#[cfg(test)]
mod tests {
    use muxr_core::PaneId;
    use muxr_core::SessionName;
    use muxr_core::TabId;

    use super::*;
    use crate::state::Pane;
    use crate::state::PaneState;
    use crate::state::PaneTree;
    use crate::state::Tab;

    #[test]
    fn test_focus_tab_when_tab_exists_updates_active_tab() -> rootcause::Result<()> {
        let mut layout = self::layout()?;

        assert2::assert!(handle_focus_tab(&mut layout, &TabId::new("tab-2")?));

        pretty_assertions::assert_eq!(layout.active_tab_id().as_ref(), "tab-2");
        Ok(())
    }

    #[test]
    fn test_focus_tab_when_tab_is_missing_keeps_active_tab() -> rootcause::Result<()> {
        let mut layout = self::layout()?;

        assert2::assert!(!handle_focus_tab(&mut layout, &TabId::new("tab-3")?));

        pretty_assertions::assert_eq!(layout.active_tab_id().as_ref(), "tab-1");
        Ok(())
    }

    fn layout() -> rootcause::Result<SessionLayout> {
        let session: SessionName = "work".parse()?;
        let tab_1 = TabId::new("tab-1")?;
        let tab_2 = TabId::new("tab-2")?;
        Ok(SessionLayout {
            active_tab: tab_1.clone(),
            entries: vec![
                self::tab(tab_1, PaneId::new("pane-1")?),
                self::tab(tab_2, PaneId::new("pane-2")?),
            ],
            session,
        })
    }

    fn tab(id: TabId, pane_id: PaneId) -> Tab {
        Tab {
            active_pane: pane_id.clone(),
            id,
            pane_tree: PaneTree::Pane(Pane {
                cmd_label: "sh".to_owned(),
                cwd: "/tmp".to_owned(),
                focus_seq: 1,
                id: pane_id,
                started_at: 1,
                state: PaneState::Running,
                title: "sh".to_owned(),
            }),
            title: "tab".to_owned(),
        }
    }
}
