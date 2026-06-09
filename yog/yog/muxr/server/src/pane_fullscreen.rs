use std::collections::BTreeMap;

use muxr_core::PaneId;
use muxr_core::TabId;
use muxr_core::TerminalSize;

use crate::pane_layout::PaneLayout;
use crate::state::SessionLayout;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PaneFullscreen {
    // Fullscreen is attached-client render state keyed by tab: it changes the visible pane layout
    // and PTY size without mutating the persisted split tree.
    panes: BTreeMap<TabId, PaneId>,
}

impl PaneFullscreen {
    pub fn toggle_active_pane(&mut self, layout: &SessionLayout) -> rootcause::Result<()> {
        let active_tab = layout.active_tab;
        let active_pane = layout.active_pane_id()?;
        if self.panes.get(&active_tab).copied() == Some(active_pane) {
            self.panes.remove(&active_tab);
        } else {
            self.panes.insert(active_tab, active_pane);
        }
        Ok(())
    }

    pub fn clear_active_tab(&mut self, layout: &SessionLayout) -> bool {
        self.panes.remove(&layout.active_tab).is_some()
    }

    pub fn pane_layout(&self, layout: &SessionLayout, size: &TerminalSize) -> rootcause::Result<PaneLayout> {
        let Some(pane_id) = self.panes.get(&layout.active_tab).copied() else {
            return layout.pane_layout(size);
        };
        if layout.active_pane_id()? != pane_id {
            return layout.pane_layout(size);
        }
        let Some(pane) = layout.pane(pane_id) else {
            return layout.pane_layout(size);
        };

        Ok(PaneLayout::single_pane(pane.id, pane.focus_seq, size))
    }

    pub fn visible_pane_id(&self, layout: &SessionLayout) -> rootcause::Result<Option<PaneId>> {
        let Some(pane_id) = self.panes.get(&layout.active_tab).copied() else {
            return Ok(None);
        };
        if layout.active_pane_id()? != pane_id || layout.pane(pane_id).is_none() {
            return Ok(None);
        }
        Ok(Some(pane_id))
    }
}

#[cfg(test)]
mod tests {
    use muxr_config::MuxrConfig;

    use super::*;
    use crate::pane_layout::PaneArea;
    use crate::pane_layout::PanePosition;
    use crate::pane_layout::PaneRegion;
    use crate::pane_layout::PaneSize;

    #[test]
    fn test_pane_fullscreen_toggle_active_pane_when_inactive_enters_fullscreen_layout() -> rootcause::Result<()> {
        let mut layout = crate::state::test_helpers::layout("default")?;
        let pane_id = layout.split_active_pane(
            MuxrConfig::default().layout,
            crate::state::test_helpers::metadata("sh", 2),
            crate::pane_split::PaneSplitAxis::Vertical,
        )?;
        let mut fullscreen = PaneFullscreen::default();

        fullscreen.toggle_active_pane(&layout)?;
        let pane_layout = fullscreen.pane_layout(&layout, &TerminalSize::new(80, 24)?)?;

        pretty_assertions::assert_eq!(fullscreen.visible_pane_id(&layout)?, Some(pane_id));
        pretty_assertions::assert_eq!(pane_layout.borders(), &[]);
        pretty_assertions::assert_eq!(
            pane_layout.regions(),
            &[PaneRegion {
                area: PaneArea {
                    origin: PanePosition { row: 0, col: 0 },
                    size: PaneSize { rows: 24, cols: 80 },
                },
                focus_seq: 2,
                id: pane_id,
            }]
        );
        Ok(())
    }

    #[test]
    fn test_pane_fullscreen_toggle_active_pane_when_active_exits_fullscreen() -> rootcause::Result<()> {
        let layout = crate::state::test_helpers::layout("default")?;
        let mut fullscreen = PaneFullscreen::default();

        fullscreen.toggle_active_pane(&layout)?;
        fullscreen.toggle_active_pane(&layout)?;

        pretty_assertions::assert_eq!(fullscreen.visible_pane_id(&layout)?, None);
        Ok(())
    }

    #[test]
    fn test_pane_fullscreen_visible_pane_id_when_focus_changed_returns_none() -> rootcause::Result<()> {
        let mut layout = crate::state::test_helpers::layout("default")?;
        layout.split_active_pane(
            MuxrConfig::default().layout,
            crate::state::test_helpers::metadata("sh", 2),
            crate::pane_split::PaneSplitAxis::Vertical,
        )?;
        let mut fullscreen = PaneFullscreen::default();
        fullscreen.toggle_active_pane(&layout)?;

        let _focused =
            layout.focus_pane_direction(&TerminalSize::new(80, 24)?, crate::pane_focus::PaneFocusDirection::Left)?;

        pretty_assertions::assert_eq!(fullscreen.visible_pane_id(&layout)?, None);
        Ok(())
    }

    #[test]
    fn test_pane_fullscreen_pane_layout_when_tab_focus_changes_keeps_original_tab_fullscreen() -> rootcause::Result<()>
    {
        let mut layout = crate::state::test_helpers::layout("default")?;
        let tab_a = layout.active_tab;
        let pane_a = layout.split_active_pane(
            MuxrConfig::default().layout,
            crate::state::test_helpers::metadata("sh", 2),
            crate::pane_split::PaneSplitAxis::Vertical,
        )?;
        let mut fullscreen = PaneFullscreen::default();
        fullscreen.toggle_active_pane(&layout)?;
        let _pane_b = layout.create_tab(crate::state::test_helpers::metadata("sh", 3))?;
        layout.split_active_pane(
            MuxrConfig::default().layout,
            crate::state::test_helpers::metadata("sh", 4),
            crate::pane_split::PaneSplitAxis::Vertical,
        )?;

        let focused_tab_layout = fullscreen.pane_layout(&layout, &TerminalSize::new(80, 24)?)?;
        layout.focus_tab(tab_a)?;
        let restored_fullscreen_layout = fullscreen.pane_layout(&layout, &TerminalSize::new(80, 24)?)?;

        pretty_assertions::assert_eq!(fullscreen.visible_pane_id(&layout)?, Some(pane_a));
        pretty_assertions::assert_eq!(focused_tab_layout.regions().len(), 2);
        pretty_assertions::assert_eq!(focused_tab_layout.borders().len(), 1);
        pretty_assertions::assert_eq!(restored_fullscreen_layout.regions()[0].id, pane_a);
        pretty_assertions::assert_eq!(restored_fullscreen_layout.borders(), &[]);
        Ok(())
    }

    #[test]
    fn test_pane_fullscreen_pane_layout_when_inactive_uses_real_layout() -> rootcause::Result<()> {
        let mut layout = crate::state::test_helpers::layout("default")?;
        layout.split_active_pane(
            MuxrConfig::default().layout,
            crate::state::test_helpers::metadata("sh", 2),
            crate::pane_split::PaneSplitAxis::Vertical,
        )?;

        let pane_layout = PaneFullscreen::default().pane_layout(&layout, &TerminalSize::new(80, 24)?)?;

        pretty_assertions::assert_eq!(pane_layout.regions().len(), 2);
        pretty_assertions::assert_eq!(pane_layout.borders().len(), 1);
        Ok(())
    }

    #[test]
    fn test_pane_fullscreen_clear_active_tab_when_multiple_tabs_are_fullscreen_keeps_inactive_tab()
    -> rootcause::Result<()> {
        let mut layout = crate::state::test_helpers::layout("default")?;
        let tab_a = layout.active_tab;
        let pane_a = layout.split_active_pane(
            MuxrConfig::default().layout,
            crate::state::test_helpers::metadata("sh", 2),
            crate::pane_split::PaneSplitAxis::Vertical,
        )?;
        let mut fullscreen = PaneFullscreen::default();
        fullscreen.toggle_active_pane(&layout)?;
        layout.create_tab(crate::state::test_helpers::metadata("sh", 3))?;
        fullscreen.toggle_active_pane(&layout)?;

        pretty_assertions::assert_eq!(fullscreen.clear_active_tab(&layout), true);
        pretty_assertions::assert_eq!(fullscreen.clear_active_tab(&layout), false);
        layout.focus_tab(tab_a)?;

        pretty_assertions::assert_eq!(fullscreen.visible_pane_id(&layout)?, Some(pane_a));
        Ok(())
    }
}
