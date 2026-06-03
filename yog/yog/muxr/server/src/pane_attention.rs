use muxr_core::PaneId;
use rootcause::report;

use crate::state::Pane;
use crate::state::PaneAttentionState;
use crate::state::SessionLayout;

impl Pane {
    pub const fn acknowledge_attention(&mut self) -> bool {
        let cleared_pane_attention = self.clear_attention();
        let acknowledged_agent_attention = self.acknowledge_agent_attention();
        cleared_pane_attention || acknowledged_agent_attention
    }

    pub const fn clear_attention(&mut self) -> bool {
        if !self.attention_state.needs_attention() {
            return false;
        }
        self.attention_state = PaneAttentionState::Idle;
        true
    }

    pub const fn needs_attention(&self) -> bool {
        self.attention_state.needs_attention() || self.agent_state.needs_attention()
    }
}

impl SessionLayout {
    pub fn acknowledge_active_pane_attention(&mut self) -> rootcause::Result<bool> {
        let active_pane = self.active_pane_id()?;
        let Some(pane) = self.pane_mut(&active_pane) else {
            return Err(
                report!("muxr active pane is missing from server layout").attach(format!("pane_id={active_pane}"))
            );
        };
        Ok(pane.acknowledge_attention())
    }

    pub fn attention_pane_ids(&self) -> Vec<PaneId> {
        // Attention is intentionally explicit. Raw PTY output is too broad because startup,
        // splits, and shell prompts would otherwise paint unfocused panes as needing attention.
        self.panes()
            .into_iter()
            .filter(|pane| pane.needs_attention())
            .map(|pane| pane.id.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use muxr_core::PaneAgentState;
    use muxr_core::SessionName;
    use muxr_core::TerminalSize;

    use super::*;
    use crate::pane_focus::PaneFocusDirection;
    use crate::pane_split::PaneSplitAxis;
    use crate::state::SessionMetadata;

    #[test]
    fn test_attention_pane_ids_when_pane_needs_generic_attention_returns_pane() -> rootcause::Result<()> {
        let mut layout = self::layout()?;
        let pane_id = PaneId::new("pane-1")?;
        let Some(pane) = layout.pane_mut(&pane_id) else {
            return Err(report!("expected pane"));
        };
        pane.attention_state = PaneAttentionState::NeedsAttention;

        pretty_assertions::assert_eq!(layout.attention_pane_ids(), vec![pane_id]);
        Ok(())
    }

    #[test]
    fn test_attention_pane_ids_when_agent_is_unseen_returns_pane() -> rootcause::Result<()> {
        let mut layout = self::layout()?;
        let pane_id = PaneId::new("pane-1")?;
        let Some(pane) = layout.pane_mut(&pane_id) else {
            return Err(report!("expected pane"));
        };
        pane.agent_state = PaneAgentState::Unseen;

        pretty_assertions::assert_eq!(layout.attention_pane_ids(), vec![pane_id]);
        Ok(())
    }

    #[test]
    fn test_focus_pane_direction_when_target_needs_generic_attention_clears_attention() -> rootcause::Result<()> {
        let mut layout = self::layout()?;
        let pane_id = PaneId::new("pane-1")?;
        let Some(pane) = layout.pane_mut(&pane_id) else {
            return Err(report!("expected pane"));
        };
        pane.attention_state = PaneAttentionState::NeedsAttention;

        assert2::assert!(layout.focus_pane_direction(&TerminalSize::new(80, 24)?, PaneFocusDirection::Left)?);

        pretty_assertions::assert_eq!(layout.attention_pane_ids(), Vec::<PaneId>::new());
        Ok(())
    }

    #[test]
    fn test_focus_pane_direction_when_agent_is_unseen_acknowledges_agent_attention() -> rootcause::Result<()> {
        let mut layout = self::layout()?;
        let pane_id = PaneId::new("pane-1")?;
        let Some(pane) = layout.pane_mut(&pane_id) else {
            return Err(report!("expected pane"));
        };
        pane.agent_state = PaneAgentState::Unseen;

        assert2::assert!(layout.focus_pane_direction(&TerminalSize::new(80, 24)?, PaneFocusDirection::Left)?);

        pretty_assertions::assert_eq!(layout.attention_pane_ids(), Vec::<PaneId>::new());
        pretty_assertions::assert_eq!(
            layout
                .pane(&pane_id)
                .ok_or_else(|| report!("expected pane"))?
                .agent_state,
            PaneAgentState::Seen
        );
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
