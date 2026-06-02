use muxr_core::PaneId;
use rootcause::report;

use crate::state::Pane;
use crate::state::SessionLayout;

impl Pane {
    pub const fn clear_attention(&mut self) -> bool {
        if !self.needs_attention {
            return false;
        }
        self.needs_attention = false;
        true
    }
}

impl SessionLayout {
    pub fn clear_active_pane_attention(&mut self) -> rootcause::Result<bool> {
        let active_pane = self.active_pane_id()?;
        let Some(pane) = self.pane_mut(&active_pane) else {
            return Err(
                report!("muxr active pane is missing from server layout").attach(format!("pane_id={active_pane}"))
            );
        };
        Ok(pane.clear_attention())
    }

    pub fn attention_pane_ids(&self) -> Vec<PaneId> {
        // Attention is intentionally explicit. Raw PTY output is too broad because startup,
        // splits, and shell prompts would otherwise paint unfocused panes as needing attention.
        self.panes()
            .into_iter()
            .filter(|pane| pane.needs_attention)
            .map(|pane| pane.id.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use muxr_core::SessionName;
    use muxr_core::TerminalSize;

    use super::*;
    use crate::pane_focus::PaneFocusDirection;
    use crate::pane_split::PaneSplitAxis;
    use crate::state::SessionMetadata;

    #[test]
    fn test_attention_pane_ids_when_pane_needs_attention_returns_pane() -> rootcause::Result<()> {
        let mut layout = self::layout()?;
        let pane_id = PaneId::new("pane-1")?;
        let Some(pane) = layout.pane_mut(&pane_id) else {
            return Err(report!("expected pane"));
        };
        pane.needs_attention = true;

        pretty_assertions::assert_eq!(layout.attention_pane_ids(), vec![pane_id]);
        Ok(())
    }

    #[test]
    fn test_focus_pane_direction_when_target_needs_attention_clears_attention() -> rootcause::Result<()> {
        let mut layout = self::layout()?;
        let pane_id = PaneId::new("pane-1")?;
        let Some(pane) = layout.pane_mut(&pane_id) else {
            return Err(report!("expected pane"));
        };
        pane.needs_attention = true;

        assert2::assert!(layout.focus_pane_direction(&TerminalSize::new(80, 24)?, PaneFocusDirection::Left)?);

        assert2::assert!(layout.attention_pane_ids().is_empty());
        Ok(())
    }

    fn layout() -> rootcause::Result<SessionLayout> {
        let session: SessionName = "work".parse()?;
        let mut layout = SessionLayout::initial(&session, self::metadata("sh", 1))?;
        layout.split_active_pane(self::metadata("sh", 2), PaneSplitAxis::Vertical)?;
        Ok(layout)
    }

    fn metadata(cmd_label: &str, started_at: u64) -> SessionMetadata {
        SessionMetadata {
            cmd_label: cmd_label.to_owned(),
            cwd: "/tmp".to_owned(),
            started_at,
        }
    }
}
