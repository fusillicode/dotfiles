use std::time::Instant;

use muxr_core::PaneAgentState;
use muxr_core::PaneId;
use rootcause::report;
use ytil_agents::agent::Agent;

use crate::pane_agent::PaneAgentProcess;
use crate::pane_agent::PaneAgentRuntime;
use crate::pane_agent::PaneUserInteraction;
use crate::state::Pane;
use crate::state::PaneAttentionState;
use crate::state::SessionLayout;

#[derive(Debug, Default)]
pub struct PaneAttentionTracker {
    agent_runtime: PaneAgentRuntime,
}

impl Pane {
    pub const fn acknowledge_attention(&mut self) -> bool {
        self.clear_attention()
    }

    pub const fn clear_attention(&mut self) -> bool {
        if !self.attention_state.needs_attention() {
            return false;
        }
        self.attention_state = PaneAttentionState::Idle;
        true
    }

    pub const fn needs_attention(&self) -> bool {
        self.attention_state.needs_attention()
    }
}

impl PaneAttentionTracker {
    pub fn sync_agent_attention(
        &mut self,
        layout: &SessionLayout,
        agent_processes: &[PaneAgentProcess],
        visible_activity_panes: &[PaneId],
        now: Instant,
    ) -> rootcause::Result<bool> {
        self.retain_layout_panes(layout);
        self.agent_runtime.discard_stale_activity(now);
        let mut changed = self.sync_agent_processes(layout, agent_processes);
        changed |= self.consume_pending_visible_activity(layout, agent_processes);
        changed |= self.record_visible_activity(layout, agent_processes, visible_activity_panes, now);
        changed |= self.mark_quiet_agent_panes(layout, agent_processes, now)?;
        Ok(changed)
    }

    pub fn acknowledge_agent_attention(&mut self, pane_id: PaneId) -> bool {
        self.agent_runtime.acknowledge_attention(pane_id)
    }

    pub fn agent_states(&self) -> Vec<(PaneId, PaneAgentState)> {
        self.agent_runtime.states()
    }

    #[cfg(test)]
    fn agent_state_for(&self, pane_id: PaneId) -> PaneAgentState {
        self.agent_runtime.state(pane_id)
    }

    pub fn attention_pane_ids(&self, layout: &SessionLayout) -> Vec<PaneId> {
        layout
            .panes()
            .into_iter()
            .filter(|pane| pane.needs_attention() || self.agent_runtime.needs_attention(pane.id))
            .map(|pane| pane.id)
            .collect()
    }

    pub fn record_user_interaction(&mut self, pane_id: PaneId, interaction: PaneUserInteraction, now: Instant) {
        self.agent_runtime.record_user_interaction(pane_id, interaction, now);
    }

    pub fn sync_agent_processes(&mut self, layout: &SessionLayout, agent_processes: &[PaneAgentProcess]) -> bool {
        self.retain_layout_panes(layout);
        let pane_ids = self::layout_pane_ids(layout);
        let mut changed = false;
        for pane_id in pane_ids {
            let agent = self::agent_for(agent_processes, pane_id);
            changed |= self.agent_runtime.sync_process(pane_id, agent);
        }
        changed
    }

    fn record_visible_activity(
        &mut self,
        layout: &SessionLayout,
        agent_processes: &[PaneAgentProcess],
        visible_activity_panes: &[PaneId],
        now: Instant,
    ) -> bool {
        let mut changed = false;
        for pane_id in visible_activity_panes {
            let agent = self::agent_for(agent_processes, *pane_id);
            if layout.pane(*pane_id).is_none() {
                continue;
            }
            changed |= self.agent_runtime.record_visible_activity(*pane_id, agent, now);
        }
        changed
    }

    fn consume_pending_visible_activity(
        &mut self,
        layout: &SessionLayout,
        agent_processes: &[PaneAgentProcess],
    ) -> bool {
        let pane_ids = self::layout_pane_ids(layout);
        let mut changed = false;
        for pane_id in pane_ids {
            let agent = self::agent_for(agent_processes, pane_id);
            changed |= self.agent_runtime.consume_pending_visible_activity(pane_id, agent);
        }
        changed
    }

    fn mark_quiet_agent_panes(
        &mut self,
        layout: &SessionLayout,
        agent_processes: &[PaneAgentProcess],
        now: Instant,
    ) -> rootcause::Result<bool> {
        let focused_pane = layout.active_pane_id()?;
        let pane_ids = self::layout_pane_ids(layout);
        let mut changed = false;
        for pane_id in pane_ids {
            let Some(agent) = self::agent_for(agent_processes, pane_id) else {
                continue;
            };
            changed |= self
                .agent_runtime
                .mark_quiet_if_due(pane_id, agent, now, pane_id == focused_pane);
        }
        Ok(changed)
    }

    fn retain_layout_panes(&mut self, layout: &SessionLayout) {
        let pane_ids = self::layout_pane_ids(layout).into_iter().collect();
        self.agent_runtime.retain_panes(&pane_ids);
    }
}

impl SessionLayout {
    pub fn acknowledge_active_pane_attention(&mut self) -> rootcause::Result<bool> {
        let active_pane = self.active_pane_id()?;
        let Some(pane) = self.pane_mut(active_pane) else {
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
            .map(|pane| pane.id)
            .collect()
    }
}

fn layout_pane_ids(layout: &SessionLayout) -> Vec<PaneId> {
    layout.panes().into_iter().map(|pane| pane.id).collect()
}

fn agent_for(agent_processes: &[PaneAgentProcess], pane_id: PaneId) -> Option<Agent> {
    agent_processes
        .iter()
        .find(|process| *process.pane_id() == pane_id)
        .and_then(PaneAgentProcess::agent)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use muxr_core::PaneAgentState;
    use muxr_core::SessionName;
    use muxr_core::TerminalSize;
    use ytil_agents::agent::Agent;

    use super::*;
    use crate::pane_focus::PaneFocusDirection;
    use crate::pane_split::PaneSplitAxis;
    use crate::state::SessionMetadata;

    #[test]
    fn test_attention_pane_ids_when_pane_needs_generic_attention_returns_pane() -> rootcause::Result<()> {
        let mut layout = self::layout()?;
        let pane_id = PaneId::new(1)?;
        let Some(pane) = layout.pane_mut(pane_id) else {
            return Err(report!("expected pane"));
        };
        pane.attention_state = PaneAttentionState::NeedsAttention;

        let tracker = PaneAttentionTracker::default();

        pretty_assertions::assert_eq!(tracker.attention_pane_ids(&layout), vec![pane_id]);
        Ok(())
    }

    #[test]
    fn test_attention_pane_ids_when_agent_is_unseen_returns_pane() -> rootcause::Result<()> {
        let layout = self::layout()?;
        let mut tracker = PaneAttentionTracker::default();
        let pane_id = PaneId::new(1)?;
        let agent_processes = self::agent_processes(1, Some(Agent::Codex))?;
        let then = Instant::now();

        assert2::assert!(tracker.sync_agent_attention(&layout, &agent_processes, &[], then)?);
        assert2::assert!(tracker.sync_agent_attention(
            &layout,
            &agent_processes,
            std::slice::from_ref(&pane_id),
            self::instant_after(then, Duration::from_secs(1))?
        )?);
        assert2::assert!(tracker.sync_agent_attention(
            &layout,
            &agent_processes,
            &[],
            self::instant_after(then, Duration::from_secs(4))?
        )?);

        pretty_assertions::assert_eq!(tracker.attention_pane_ids(&layout), vec![pane_id]);
        Ok(())
    }

    #[test]
    fn test_focus_pane_direction_when_target_needs_generic_attention_clears_attention() -> rootcause::Result<()> {
        let mut layout = self::layout()?;
        let pane_id = PaneId::new(1)?;
        let Some(pane) = layout.pane_mut(pane_id) else {
            return Err(report!("expected pane"));
        };
        pane.attention_state = PaneAttentionState::NeedsAttention;

        assert2::assert!(layout.focus_pane_direction(&TerminalSize::new(80, 24)?, PaneFocusDirection::Left)?);

        let tracker = PaneAttentionTracker::default();

        pretty_assertions::assert_eq!(tracker.attention_pane_ids(&layout), Vec::<PaneId>::new());
        Ok(())
    }

    #[test]
    fn test_acknowledge_agent_attention_when_agent_is_unseen_marks_agent_seen() -> rootcause::Result<()> {
        let layout = self::layout()?;
        let mut tracker = PaneAttentionTracker::default();
        let pane_id = PaneId::new(1)?;
        let agent_processes = self::agent_processes(1, Some(Agent::Codex))?;
        let then = Instant::now();

        assert2::assert!(tracker.sync_agent_attention(&layout, &agent_processes, &[], then)?);
        assert2::assert!(tracker.sync_agent_attention(
            &layout,
            &agent_processes,
            std::slice::from_ref(&pane_id),
            self::instant_after(then, Duration::from_secs(1))?
        )?);
        assert2::assert!(tracker.sync_agent_attention(
            &layout,
            &agent_processes,
            &[],
            self::instant_after(then, Duration::from_secs(4))?
        )?);

        assert2::assert!(tracker.acknowledge_agent_attention(pane_id));

        pretty_assertions::assert_eq!(tracker.attention_pane_ids(&layout), Vec::<PaneId>::new());
        pretty_assertions::assert_eq!(tracker.agent_state_for(pane_id), PaneAgentState::Seen);
        Ok(())
    }

    #[test]
    fn test_pane_attention_tracker_when_agent_has_visible_activity_and_then_is_quiet_unfocused_marks_unseen()
    -> rootcause::Result<()> {
        let layout = self::layout()?;
        let mut tracker = PaneAttentionTracker::default();
        let pane_id = PaneId::new(1)?;
        let agent_processes = self::agent_processes(1, Some(Agent::Codex))?;
        let then = Instant::now();

        assert2::assert!(tracker.sync_agent_attention(&layout, &agent_processes, &[], then)?);
        assert2::assert!(tracker.sync_agent_attention(
            &layout,
            &agent_processes,
            std::slice::from_ref(&pane_id),
            self::instant_after(then, Duration::from_secs(1))?
        )?);
        assert2::assert!(tracker.sync_agent_attention(
            &layout,
            &agent_processes,
            &[],
            self::instant_after(then, Duration::from_secs(4))?
        )?);

        pretty_assertions::assert_eq!(tracker.agent_state_for(pane_id), PaneAgentState::Unseen);
        Ok(())
    }

    #[rstest::rstest]
    #[case::recent_pending_activity(Duration::from_millis(100), PaneAgentState::Busy)]
    #[case::stale_pending_activity(Duration::from_millis(501), PaneAgentState::Seen)]
    fn test_pane_attention_tracker_when_agent_detection_lags_visible_activity_uses_only_recent_pending_activity(
        #[case] detection_delay: Duration,
        #[case] expected_agent_state: PaneAgentState,
    ) -> rootcause::Result<()> {
        let layout = self::layout()?;
        let mut tracker = PaneAttentionTracker::default();
        let pane_id = PaneId::new(1)?;
        let no_agent = self::agent_processes(1, None)?;
        let agent_processes = self::agent_processes(1, Some(Agent::Codex))?;
        let then = Instant::now();

        assert2::assert!(!tracker.sync_agent_attention(&layout, &no_agent, std::slice::from_ref(&pane_id), then,)?);
        let detected_at = self::instant_after(then, detection_delay)?;
        assert2::assert!(tracker.sync_agent_attention(&layout, &agent_processes, &[], detected_at)?);

        pretty_assertions::assert_eq!(tracker.agent_state_for(pane_id), expected_agent_state);
        let quiet_at = self::instant_after(then, Duration::from_secs(4))?;
        if expected_agent_state == PaneAgentState::Busy {
            assert2::assert!(tracker.sync_agent_attention(&layout, &agent_processes, &[], quiet_at)?);
            pretty_assertions::assert_eq!(tracker.agent_state_for(pane_id), PaneAgentState::Unseen);
        } else {
            assert2::assert!(!tracker.sync_agent_attention(&layout, &agent_processes, &[], quiet_at)?);
            pretty_assertions::assert_eq!(tracker.attention_pane_ids(&layout), Vec::<PaneId>::new());
        }
        Ok(())
    }

    #[test]
    fn test_pane_attention_tracker_when_agent_has_no_visible_activity_does_not_mark_attention() -> rootcause::Result<()>
    {
        let layout = self::layout()?;
        let mut tracker = PaneAttentionTracker::default();
        let agent_processes = self::agent_processes(1, Some(Agent::Codex))?;
        let then = Instant::now();

        assert2::assert!(tracker.sync_agent_attention(&layout, &agent_processes, &[], then)?);
        assert2::assert!(!tracker.sync_agent_attention(
            &layout,
            &agent_processes,
            &[],
            self::instant_after(then, Duration::from_secs(3))?
        )?);

        pretty_assertions::assert_eq!(tracker.attention_pane_ids(&layout), Vec::<PaneId>::new());
        Ok(())
    }

    #[rstest::rstest]
    #[case::recent_user_interaction(Duration::from_millis(100), PaneAgentState::Seen, false)]
    #[case::stale_user_interaction(Duration::from_millis(501), PaneAgentState::Busy, true)]
    fn test_pane_attention_tracker_when_user_interaction_echoes_visible_activity_does_not_mark_agent_busy(
        #[case] visible_activity_delay: Duration,
        #[case] expected_agent_state: PaneAgentState,
        #[case] expected_changed: bool,
    ) -> rootcause::Result<()> {
        let layout = self::layout()?;
        let mut tracker = PaneAttentionTracker::default();
        let pane_id = PaneId::new(1)?;
        let agent_processes = self::agent_processes(1, Some(Agent::Codex))?;
        let then = Instant::now();

        assert2::assert!(tracker.sync_agent_attention(&layout, &agent_processes, &[], then)?);
        tracker.record_user_interaction(pane_id, PaneUserInteraction::MayEcho, then);

        let changed = tracker.sync_agent_attention(
            &layout,
            &agent_processes,
            std::slice::from_ref(&pane_id),
            self::instant_after(then, visible_activity_delay)?,
        )?;

        pretty_assertions::assert_eq!(changed, expected_changed);
        pretty_assertions::assert_eq!(tracker.agent_state_for(pane_id), expected_agent_state);
        Ok(())
    }

    #[test]
    fn test_pane_attention_tracker_when_prompt_submit_follows_typing_keeps_fast_agent_activity() -> rootcause::Result<()>
    {
        let layout = self::layout()?;
        let mut tracker = PaneAttentionTracker::default();
        let pane_id = PaneId::new(1)?;
        let agent_processes = self::agent_processes(1, Some(Agent::Codex))?;
        let then = Instant::now();

        assert2::assert!(tracker.sync_agent_attention(&layout, &agent_processes, &[], then)?);
        tracker.record_user_interaction(pane_id, PaneUserInteraction::MayEcho, then);
        tracker.record_user_interaction(
            pane_id,
            PaneUserInteraction::StartsAgentWork,
            self::instant_after(then, Duration::from_millis(100))?,
        );
        assert2::assert!(tracker.sync_agent_attention(
            &layout,
            &agent_processes,
            std::slice::from_ref(&pane_id),
            self::instant_after(then, Duration::from_millis(150))?,
        )?);

        pretty_assertions::assert_eq!(tracker.agent_state_for(pane_id), PaneAgentState::Busy);
        Ok(())
    }

    #[test]
    fn test_pane_attention_tracker_when_mouse_interaction_redraws_does_not_mark_agent_busy() -> rootcause::Result<()> {
        let layout = self::layout()?;
        let mut tracker = PaneAttentionTracker::default();
        let pane_id = PaneId::new(1)?;
        let agent_processes = self::agent_processes(1, Some(Agent::Codex))?;
        let then = Instant::now();

        assert2::assert!(tracker.sync_agent_attention(&layout, &agent_processes, &[], then)?);
        tracker.record_user_interaction(pane_id, PaneUserInteraction::MayEcho, then);
        assert2::assert!(!tracker.sync_agent_attention(
            &layout,
            &agent_processes,
            std::slice::from_ref(&pane_id),
            self::instant_after(then, Duration::from_millis(100))?,
        )?);

        pretty_assertions::assert_eq!(tracker.agent_state_for(pane_id), PaneAgentState::Seen);
        Ok(())
    }

    #[test]
    fn test_pane_attention_tracker_when_agent_identity_changes_resets_activity_and_state() -> rootcause::Result<()> {
        let layout = self::layout()?;
        let mut tracker = PaneAttentionTracker::default();
        let pane_id = PaneId::new(1)?;
        let codex = self::agent_processes(1, Some(Agent::Codex))?;
        let cursor = self::agent_processes(1, Some(Agent::Cursor))?;
        let then = Instant::now();

        assert2::assert!(tracker.sync_agent_attention(&layout, &codex, &[], then)?);
        assert2::assert!(tracker.sync_agent_attention(
            &layout,
            &codex,
            std::slice::from_ref(&pane_id),
            self::instant_after(then, Duration::from_millis(100))?,
        )?);
        pretty_assertions::assert_eq!(tracker.agent_state_for(pane_id), PaneAgentState::Busy);

        assert2::assert!(tracker.sync_agent_attention(
            &layout,
            &cursor,
            &[],
            self::instant_after(then, Duration::from_millis(200))?,
        )?);
        pretty_assertions::assert_eq!(tracker.agent_state_for(pane_id), PaneAgentState::Seen);
        assert2::assert!(!tracker.sync_agent_attention(
            &layout,
            &cursor,
            &[],
            self::instant_after(then, Duration::from_secs(4))?,
        )?);
        pretty_assertions::assert_eq!(tracker.attention_pane_ids(&layout), Vec::<PaneId>::new());
        Ok(())
    }

    #[test]
    fn test_pane_attention_tracker_when_fresh_tracker_observes_running_agent_starts_seen() -> rootcause::Result<()> {
        let layout = self::layout()?;
        let mut tracker = PaneAttentionTracker::default();
        let pane_id = PaneId::new(1)?;
        let agent_processes = self::agent_processes(1, Some(Agent::Codex))?;
        let then = Instant::now();

        assert2::assert!(tracker.sync_agent_attention(&layout, &agent_processes, &[], then)?);
        assert2::assert!(!tracker.sync_agent_attention(
            &layout,
            &agent_processes,
            &[],
            self::instant_after(then, Duration::from_secs(3))?
        )?);

        pretty_assertions::assert_eq!(tracker.agent_state_for(pane_id), PaneAgentState::Seen);
        pretty_assertions::assert_eq!(tracker.attention_pane_ids(&layout), Vec::<PaneId>::new());
        Ok(())
    }

    #[test]
    fn test_pane_attention_tracker_when_visible_activity_is_quiet_and_focused_marks_seen() -> rootcause::Result<()> {
        let layout = self::layout()?;
        let mut tracker = PaneAttentionTracker::default();
        let pane_id = PaneId::new(2)?;
        let agent_processes = self::agent_processes(2, Some(Agent::Codex))?;
        let then = Instant::now();

        assert2::assert!(tracker.sync_agent_attention(&layout, &agent_processes, &[], then)?);
        assert2::assert!(tracker.sync_agent_attention(
            &layout,
            &agent_processes,
            std::slice::from_ref(&pane_id),
            self::instant_after(then, Duration::from_secs(1))?
        )?);
        assert2::assert!(!tracker.sync_agent_attention(
            &layout,
            &agent_processes,
            &[],
            self::instant_after(then, Duration::from_secs(2))?
        )?);
        assert2::assert!(tracker.sync_agent_attention(
            &layout,
            &agent_processes,
            &[],
            self::instant_after(then, Duration::from_secs(4))?
        )?);

        pretty_assertions::assert_eq!(tracker.agent_state_for(pane_id), PaneAgentState::Seen);
        Ok(())
    }

    #[test]
    fn test_pane_attention_tracker_when_unseen_agent_has_visible_activity_marks_busy() -> rootcause::Result<()> {
        let layout = self::layout()?;
        let mut tracker = PaneAttentionTracker::default();
        let pane_id = PaneId::new(1)?;
        let agent_processes = self::agent_processes(1, Some(Agent::Codex))?;
        let then = Instant::now();

        assert2::assert!(tracker.sync_agent_attention(&layout, &agent_processes, &[], then)?);
        assert2::assert!(tracker.sync_agent_attention(
            &layout,
            &agent_processes,
            std::slice::from_ref(&pane_id),
            self::instant_after(then, Duration::from_secs(1))?,
        )?);
        assert2::assert!(tracker.sync_agent_attention(
            &layout,
            &agent_processes,
            &[],
            self::instant_after(then, Duration::from_secs(4))?,
        )?);

        assert2::assert!(tracker.sync_agent_attention(
            &layout,
            &agent_processes,
            std::slice::from_ref(&pane_id),
            self::instant_after(then, Duration::from_secs(5))?,
        )?);

        pretty_assertions::assert_eq!(tracker.agent_state_for(pane_id), PaneAgentState::Busy);
        Ok(())
    }

    #[test]
    fn test_pane_attention_tracker_when_shell_output_is_quiet_does_not_mark_attention() -> rootcause::Result<()> {
        let layout = self::layout()?;
        let mut tracker = PaneAttentionTracker::default();
        let pane_id = PaneId::new(1)?;
        let agent_processes = self::agent_processes(1, None)?;
        let then = Instant::now();

        assert2::assert!(!tracker.sync_agent_attention(
            &layout,
            &agent_processes,
            std::slice::from_ref(&pane_id),
            then,
        )?);
        assert2::assert!(!tracker.sync_agent_attention(
            &layout,
            &agent_processes,
            &[],
            self::instant_after(then, Duration::from_secs(3))?
        )?);

        pretty_assertions::assert_eq!(tracker.attention_pane_ids(&layout), Vec::<PaneId>::new());
        Ok(())
    }

    #[test]
    fn test_pane_attention_tracker_when_agent_process_exits_clears_agent_state() -> rootcause::Result<()> {
        let layout = self::layout()?;
        let mut tracker = PaneAttentionTracker::default();
        let pane_id = PaneId::new(1)?;
        let agent_processes = self::agent_processes(1, Some(Agent::Codex))?;
        let no_agent = self::agent_processes(1, None)?;
        let then = Instant::now();

        assert2::assert!(tracker.sync_agent_attention(&layout, &agent_processes, &[], then)?);
        assert2::assert!(tracker.sync_agent_attention(
            &layout,
            &agent_processes,
            std::slice::from_ref(&pane_id),
            self::instant_after(then, Duration::from_secs(1))?
        )?);
        assert2::assert!(tracker.sync_agent_attention(
            &layout,
            &agent_processes,
            &[],
            self::instant_after(then, Duration::from_secs(4))?
        )?);

        assert2::assert!(tracker.sync_agent_attention(
            &layout,
            &no_agent,
            &[],
            self::instant_after(then, Duration::from_secs(5))?
        )?);

        pretty_assertions::assert_eq!(tracker.agent_state_for(pane_id), PaneAgentState::NoAgent);
        pretty_assertions::assert_eq!(tracker.attention_pane_ids(&layout), Vec::<PaneId>::new());
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

    fn agent_processes(pane_id: u32, agent: Option<Agent>) -> rootcause::Result<Vec<PaneAgentProcess>> {
        let pane_id = PaneId::new(pane_id)?;
        Ok(vec![agent.map_or(PaneAgentProcess::NoAgent { pane_id }, |agent| {
            PaneAgentProcess::Agent { pane_id, agent }
        })])
    }

    fn instant_after(instant: Instant, duration: Duration) -> rootcause::Result<Instant> {
        instant
            .checked_add(duration)
            .ok_or_else(|| report!("test instant overflowed"))
    }
}
