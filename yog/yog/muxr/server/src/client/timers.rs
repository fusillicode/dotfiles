use std::collections::BTreeSet;
use std::pin::Pin;
use std::time::Duration;
use std::time::Instant;

use muxr_core::PaneId;
use rootcause::report;

use crate::pane::tracked_process::PaneTrackedProcesses;
use crate::server::ServerConfig;
use crate::state::SessionLayout;

const CMD_HANDOFF_SAMPLE_DELAY: Duration = Duration::from_millis(50);
const INTERACTIVE_RENDER_BOOST_FOR: Duration = Duration::from_millis(250);
const INTERACTIVE_RENDER_FRAME_INTERVAL: Duration = Duration::from_millis(10);
const RENDER_FRAME_INTERVAL: Duration = Duration::from_millis(16);
const SLEEP_DISABLED_FOR: Duration = Duration::from_hours(24);
const TRACKED_PROCESS_QUIET_SETTLE_DELAY: Duration = Duration::from_millis(10);

pub struct ClientTimers {
    pub cmd_handoff_sample: Pin<Box<tokio::time::Sleep>>,
    cmd_handoff_sample_panes: BTreeSet<PaneId>,
    interactive_render_until: Option<tokio::time::Instant>,
    last_render_at: Option<tokio::time::Instant>,
    render_deadline: Option<tokio::time::Instant>,
    pub render_sleep: Pin<Box<tokio::time::Sleep>>,
    tracked_process_quiet_deadline: Option<Instant>,
    pub tracked_process_quiet_sleep: Pin<Box<tokio::time::Sleep>>,
    pub heartbeat: tokio::time::Interval,
}

impl ClientTimers {
    pub fn new(config: &ServerConfig) -> rootcause::Result<Self> {
        let heartbeat_start = tokio::time::Instant::now()
            .checked_add(config.client_heartbeat_interval)
            .ok_or_else(|| report!("muxr heartbeat interval overflowed"))?;

        Ok(Self {
            cmd_handoff_sample: Box::pin(tokio::time::sleep_until(self::disabled_sleep_deadline()?)),
            cmd_handoff_sample_panes: BTreeSet::new(),
            interactive_render_until: None,
            last_render_at: None,
            render_deadline: None,
            render_sleep: Box::pin(tokio::time::sleep_until(self::disabled_sleep_deadline()?)),
            tracked_process_quiet_deadline: None,
            tracked_process_quiet_sleep: Box::pin(tokio::time::sleep_until(self::disabled_sleep_deadline()?)),
            heartbeat: tokio::time::interval_at(heartbeat_start, config.client_heartbeat_interval),
        })
    }

    pub fn schedule_cmd_handoff_sample(&mut self, pane_id: PaneId) -> rootcause::Result<()> {
        let deadline = tokio::time::Instant::now()
            .checked_add(CMD_HANDOFF_SAMPLE_DELAY)
            .ok_or_else(|| report!("muxr cmd handoff sample deadline overflowed"))?;
        self.cmd_handoff_sample.as_mut().reset(deadline);
        self.cmd_handoff_sample_panes.insert(pane_id);
        Ok(())
    }

    pub fn remove_cmd_handoff_sample_pane(&mut self, pane_id: PaneId) -> rootcause::Result<()> {
        self.cmd_handoff_sample_panes.remove(&pane_id);
        if self.cmd_handoff_sample_panes.is_empty() {
            self.cmd_handoff_sample.as_mut().reset(self::disabled_sleep_deadline()?);
        }
        Ok(())
    }

    pub fn take_cmd_handoff_sample_panes(&mut self) -> rootcause::Result<Vec<PaneId>> {
        let pane_ids = std::mem::take(&mut self.cmd_handoff_sample_panes).into_iter().collect();
        // `tokio::time::Sleep` stays ready after it fires. Disable the one-shot immediately after consuming it so
        // the attached-client select loop cannot hot-spin and starve PTY rendering after a prompt submit.
        self.cmd_handoff_sample.as_mut().reset(self::disabled_sleep_deadline()?);
        Ok(pane_ids)
    }

    pub fn sync_render_deadline(&mut self, render_dirty: bool) -> rootcause::Result<()> {
        if !render_dirty {
            if self.render_deadline.is_some() {
                self.disable_render_sleep()?;
            }
            return Ok(());
        }
        let Some(current_deadline) = self.render_deadline else {
            return self.schedule_render_frame();
        };
        let next_deadline = self.next_render_deadline(tokio::time::Instant::now())?;
        if next_deadline < current_deadline {
            self.set_render_deadline(next_deadline);
        }
        Ok(())
    }

    pub fn record_interactive_input(&mut self) -> rootcause::Result<()> {
        let now = tokio::time::Instant::now();
        let interactive_render_until = now
            .checked_add(INTERACTIVE_RENDER_BOOST_FOR)
            .ok_or_else(|| report!("muxr interactive render boost deadline overflowed"))?;
        self.interactive_render_until = Some(interactive_render_until);
        if let Some(current_deadline) = self.render_deadline {
            let next_deadline = self.next_render_deadline(now)?;
            if next_deadline < current_deadline {
                self.set_render_deadline(next_deadline);
            }
        }
        Ok(())
    }

    // Regression context: scheduling every dirty frame at `now + RENDER_FRAME_INTERVAL` made key echo wait a full
    // extra frame after PTY output arrived. Keep the low-wakeup model by rendering the first dirty frame after idle
    // immediately, then rate-limit follow-up frames from the last render attempt. The 10ms cap is limited to a short
    // post-input window so key echo can feel closer to Zellij without making bulk output render at 100fps forever.
    // Avoid a separate user-input deadline path; the adaptive cap keeps one scheduler and tags only recent PTY-bound
    // input as latency-sensitive.
    fn schedule_render_frame(&mut self) -> rootcause::Result<()> {
        let deadline = self.next_render_deadline(tokio::time::Instant::now())?;
        self.set_render_deadline(deadline);
        Ok(())
    }

    fn next_render_deadline(&self, now: tokio::time::Instant) -> rootcause::Result<tokio::time::Instant> {
        let Some(last_render_at) = self.last_render_at else {
            return Ok(now);
        };
        let frame_interval = self.render_frame_interval(now);
        let rate_limited_deadline = last_render_at
            .checked_add(frame_interval)
            .ok_or_else(|| report!("muxr render frame deadline overflowed"))?;
        Ok(if rate_limited_deadline > now {
            rate_limited_deadline
        } else {
            now
        })
    }

    fn render_frame_interval(&self, now: tokio::time::Instant) -> Duration {
        match self.interactive_render_until {
            Some(deadline) if deadline >= now => INTERACTIVE_RENDER_FRAME_INTERVAL,
            _ => RENDER_FRAME_INTERVAL,
        }
    }

    pub fn complete_render_frame(&mut self) -> rootcause::Result<()> {
        // First dirty frame after idle renders immediately; completed frames move the next deadline forward so
        // continuous output remains capped by the normal frame interval.
        self.last_render_at = Some(tokio::time::Instant::now());
        self.disable_render_sleep()
    }

    pub fn disable_render_sleep(&mut self) -> rootcause::Result<()> {
        self.render_deadline = None;
        self.render_sleep.as_mut().reset(self::disabled_sleep_deadline()?);
        Ok(())
    }

    fn set_render_deadline(&mut self, deadline: tokio::time::Instant) {
        self.render_deadline = Some(deadline);
        self.render_sleep.as_mut().reset(deadline);
    }

    fn sync_tracked_process_quiet_deadline(&mut self, deadline: Option<Instant>) -> rootcause::Result<()> {
        if self.tracked_process_quiet_deadline == deadline {
            return Ok(());
        }

        self.tracked_process_quiet_deadline = deadline;
        let deadline = deadline.map_or_else(self::disabled_sleep_deadline, |deadline| {
            // Socket/PTY readiness can lag one poll behind an already-ready sleep. Fire the one-shot slightly after the
            // logical quiet deadline so boundary input/output gets a normal select turn before we clear Busy.
            let deadline = deadline
                .checked_add(TRACKED_PROCESS_QUIET_SETTLE_DELAY)
                .ok_or_else(|| report!("muxr tracked-process quiet sleep deadline overflowed"))?;
            Ok(tokio::time::Instant::from_std(deadline))
        })?;
        self.tracked_process_quiet_sleep.as_mut().reset(deadline);
        Ok(())
    }

    pub fn sync_tracked_process_quiet_deadline_for_layout(
        &mut self,
        pane_tracked_processes: &PaneTrackedProcesses,
        layout: &SessionLayout,
    ) -> rootcause::Result<()> {
        // The quiet deadline is focus-sensitive: focused input can extend Busy, while unfocused panes use only tracked
        // output/activity. Resync after active-pane changes so the sleep cannot keep using the old focused deadline.
        self.sync_tracked_process_quiet_deadline(pane_tracked_processes.next_quiet_deadline(layout)?)
    }

    pub fn disable_tracked_process_quiet_sleep(&mut self) -> rootcause::Result<()> {
        self.tracked_process_quiet_deadline = None;
        self.tracked_process_quiet_sleep
            .as_mut()
            .reset(self::disabled_sleep_deadline()?);
        Ok(())
    }

    pub fn tracked_process_quiet_sleep_deadline_has_passed(&self) -> bool {
        tokio::time::Instant::now() >= self.tracked_process_quiet_sleep.deadline()
    }
}

fn disabled_sleep_deadline() -> rootcause::Result<tokio::time::Instant> {
    tokio::time::Instant::now()
        .checked_add(SLEEP_DISABLED_FOR)
        .ok_or_else(|| report!("muxr disabled timer deadline overflowed"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_client_timers_when_cmd_handoff_sample_is_taken_disables_sleep() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let config = crate::server::test_helpers::server_config(tempdir.path(), "work")?;
        let pane_id = PaneId::new(1)?;
        let mut timers = ClientTimers::new(&config)?;

        timers.schedule_cmd_handoff_sample(pane_id)?;
        let scheduled_deadline = timers.cmd_handoff_sample.deadline();

        pretty_assertions::assert_eq!(timers.take_cmd_handoff_sample_panes()?, vec![pane_id]);

        assert2::assert!(timers.cmd_handoff_sample_panes.is_empty());
        assert2::assert!(timers.cmd_handoff_sample.deadline() > scheduled_deadline);
        Ok(())
    }

    #[tokio::test]
    async fn test_client_timers_when_multiple_cmd_handoffs_are_pending_returns_all_panes() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let config = crate::server::test_helpers::server_config(tempdir.path(), "work")?;
        let pane_1 = PaneId::new(1)?;
        let pane_2 = PaneId::new(2)?;
        let mut timers = ClientTimers::new(&config)?;

        timers.schedule_cmd_handoff_sample(pane_2)?;
        timers.schedule_cmd_handoff_sample(pane_1)?;

        pretty_assertions::assert_eq!(timers.take_cmd_handoff_sample_panes()?, vec![pane_1, pane_2]);
        assert2::assert!(timers.cmd_handoff_sample_panes.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn test_client_timers_when_pending_cmd_handoff_pane_is_removed_drops_sample() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let config = crate::server::test_helpers::server_config(tempdir.path(), "work")?;
        let pane_1 = PaneId::new(1)?;
        let pane_2 = PaneId::new(2)?;
        let mut timers = ClientTimers::new(&config)?;

        timers.schedule_cmd_handoff_sample(pane_1)?;
        timers.schedule_cmd_handoff_sample(pane_2)?;
        timers.remove_cmd_handoff_sample_pane(pane_1)?;

        pretty_assertions::assert_eq!(timers.take_cmd_handoff_sample_panes()?, vec![pane_2]);
        assert2::assert!(timers.cmd_handoff_sample_panes.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn test_client_timers_when_render_is_clean_keeps_render_sleep_disabled() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let config = crate::server::test_helpers::server_config(tempdir.path(), "work")?;
        let mut timers = ClientTimers::new(&config)?;
        let threshold = tokio::time::Instant::now()
            .checked_add(Duration::from_hours(23))
            .ok_or_else(|| report!("muxr test threshold overflowed"))?;

        timers.sync_render_deadline(false)?;

        pretty_assertions::assert_eq!(timers.render_deadline, None);
        assert2::assert!(timers.render_sleep.deadline() > threshold);
        Ok(())
    }

    #[tokio::test(start_paused = true)]
    async fn test_client_timers_when_render_becomes_dirty_after_idle_schedules_immediate_frame() -> rootcause::Result<()>
    {
        let tempdir = tempfile::tempdir()?;
        let config = crate::server::test_helpers::server_config(tempdir.path(), "work")?;
        let mut timers = ClientTimers::new(&config)?;
        let disabled_deadline = timers.render_sleep.deadline();

        let earliest_deadline = tokio::time::Instant::now();
        timers.sync_render_deadline(true)?;
        let latest_deadline = tokio::time::Instant::now();

        let scheduled_deadline = timers.render_sleep.deadline();
        pretty_assertions::assert_eq!(timers.render_deadline, Some(scheduled_deadline));
        assert2::assert!(scheduled_deadline >= earliest_deadline);
        assert2::assert!(scheduled_deadline <= latest_deadline);
        assert2::assert!(scheduled_deadline < disabled_deadline);
        Ok(())
    }

    #[tokio::test]
    async fn test_client_timers_when_render_stays_dirty_keeps_existing_deadline() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let config = crate::server::test_helpers::server_config(tempdir.path(), "work")?;
        let mut timers = ClientTimers::new(&config)?;

        timers.sync_render_deadline(true)?;
        let scheduled_deadline = timers.render_sleep.deadline();
        timers.sync_render_deadline(true)?;

        pretty_assertions::assert_eq!(timers.render_sleep.deadline(), scheduled_deadline);
        pretty_assertions::assert_eq!(timers.render_deadline, Some(scheduled_deadline));
        Ok(())
    }

    #[tokio::test(start_paused = true)]
    async fn test_client_timers_when_render_recently_flushed_rate_limits_next_dirty_frame() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let config = crate::server::test_helpers::server_config(tempdir.path(), "work")?;
        let mut timers = ClientTimers::new(&config)?;

        timers.sync_render_deadline(true)?;
        let earliest_deadline = tokio::time::Instant::now()
            .checked_add(RENDER_FRAME_INTERVAL)
            .ok_or_else(|| report!("muxr test render deadline overflowed"))?;
        timers.complete_render_frame()?;
        timers.sync_render_deadline(true)?;
        let latest_deadline = tokio::time::Instant::now()
            .checked_add(RENDER_FRAME_INTERVAL)
            .ok_or_else(|| report!("muxr test render deadline overflowed"))?;

        let scheduled_deadline = timers.render_sleep.deadline();
        pretty_assertions::assert_eq!(timers.render_deadline, Some(scheduled_deadline));
        assert2::assert!(scheduled_deadline >= earliest_deadline);
        assert2::assert!(scheduled_deadline <= latest_deadline);
        Ok(())
    }

    #[tokio::test(start_paused = true)]
    async fn test_client_timers_when_interactive_input_recently_arrived_uses_interactive_frame_interval()
    -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let config = crate::server::test_helpers::server_config(tempdir.path(), "work")?;
        let mut timers = ClientTimers::new(&config)?;

        timers.sync_render_deadline(true)?;
        let earliest_deadline = tokio::time::Instant::now()
            .checked_add(INTERACTIVE_RENDER_FRAME_INTERVAL)
            .ok_or_else(|| report!("muxr test interactive render deadline overflowed"))?;
        timers.complete_render_frame()?;
        timers.record_interactive_input()?;
        timers.sync_render_deadline(true)?;
        let latest_deadline = tokio::time::Instant::now()
            .checked_add(INTERACTIVE_RENDER_FRAME_INTERVAL)
            .ok_or_else(|| report!("muxr test interactive render deadline overflowed"))?;

        let scheduled_deadline = timers.render_sleep.deadline();
        pretty_assertions::assert_eq!(timers.render_deadline, Some(scheduled_deadline));
        assert2::assert!(scheduled_deadline >= earliest_deadline);
        assert2::assert!(scheduled_deadline <= latest_deadline);
        Ok(())
    }

    #[tokio::test]
    async fn test_client_timers_when_interactive_input_is_expired_uses_bulk_frame_interval() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let config = crate::server::test_helpers::server_config(tempdir.path(), "work")?;
        let mut timers = ClientTimers::new(&config)?;
        let now = tokio::time::Instant::now();
        timers.last_render_at = Some(now);
        timers.interactive_render_until = Some(
            now.checked_sub(Duration::from_millis(1))
                .ok_or_else(|| report!("muxr test interactive render deadline underflowed"))?,
        );

        let expected_deadline = now
            .checked_add(RENDER_FRAME_INTERVAL)
            .ok_or_else(|| report!("muxr test render deadline overflowed"))?;

        pretty_assertions::assert_eq!(timers.next_render_deadline(now)?, expected_deadline);
        Ok(())
    }

    #[tokio::test(start_paused = true)]
    async fn test_client_timers_when_interactive_input_arrives_shortens_pending_bulk_deadline() -> rootcause::Result<()>
    {
        let tempdir = tempfile::tempdir()?;
        let config = crate::server::test_helpers::server_config(tempdir.path(), "work")?;
        let mut timers = ClientTimers::new(&config)?;
        timers.last_render_at = Some(tokio::time::Instant::now());

        timers.sync_render_deadline(true)?;
        let bulk_deadline = timers.render_sleep.deadline();
        timers.record_interactive_input()?;
        timers.sync_render_deadline(true)?;

        let interactive_deadline = timers.render_sleep.deadline();
        pretty_assertions::assert_eq!(timers.render_deadline, Some(interactive_deadline));
        assert2::assert!(interactive_deadline < bulk_deadline);
        Ok(())
    }

    #[tokio::test]
    async fn test_client_timers_when_render_flushes_disables_render_sleep() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let config = crate::server::test_helpers::server_config(tempdir.path(), "work")?;
        let mut timers = ClientTimers::new(&config)?;

        timers.sync_render_deadline(true)?;
        let scheduled_deadline = timers.render_sleep.deadline();
        timers.complete_render_frame()?;

        pretty_assertions::assert_eq!(timers.render_deadline, None);
        assert2::assert!(timers.render_sleep.deadline() > scheduled_deadline);
        Ok(())
    }

    #[tokio::test]
    async fn test_client_timers_when_tracked_process_quiet_sleep_is_disabled_resets_without_deadline()
    -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let config = crate::server::test_helpers::server_config(tempdir.path(), "work")?;
        let mut timers = ClientTimers::new(&config)?;
        let disabled_deadline = timers.tracked_process_quiet_sleep.deadline();

        timers.disable_tracked_process_quiet_sleep()?;

        pretty_assertions::assert_eq!(timers.tracked_process_quiet_deadline, None);
        assert2::assert!(timers.tracked_process_quiet_sleep.deadline() > disabled_deadline);
        Ok(())
    }

    #[tokio::test(start_paused = true)]
    async fn test_client_timers_when_active_pane_removal_drops_tracked_process_quiet_deadline() -> rootcause::Result<()>
    {
        let tempdir = tempfile::tempdir()?;
        let config = crate::server::test_helpers::server_config(tempdir.path(), "work")?;
        let mut timers = ClientTimers::new(&config)?;
        let mut layout = self::layout(&config)?;
        let pane_id = PaneId::new(1)?;
        let other_pane_id = PaneId::new(2)?;
        layout.active_tab_mut()?.focus_pane(pane_id)?;
        let mut pane_tracked_processes = PaneTrackedProcesses::default();
        let then = Instant::now();
        pane_tracked_processes.observe_pane_cmd(
            config.user_config.as_ref(),
            pane_id,
            &self::fg_tracked_process("codex"),
            then,
        );
        pane_tracked_processes.record_user_interaction(
            &layout,
            pane_id,
            crate::pane::tracked_process::TrackedProcessUserInteraction::MayEcho,
            self::instant_after(then, Duration::from_secs(2))?,
        )?;

        timers.sync_tracked_process_quiet_deadline_for_layout(&pane_tracked_processes, &layout)?;
        pretty_assertions::assert_eq!(
            timers.tracked_process_quiet_deadline,
            Some(self::instant_after(then, Duration::from_secs(5))?)
        );
        pretty_assertions::assert_eq!(
            timers.tracked_process_quiet_sleep.deadline(),
            self::tracked_process_quiet_sleep_deadline(then, Duration::from_secs(5))?
        );
        let focused_deadline = timers.tracked_process_quiet_sleep.deadline();

        layout.remove_exited_pane(pane_id, 0, self::successful_exit_status())?;
        pretty_assertions::assert_eq!(layout.active_pane_id()?, other_pane_id);
        timers.sync_tracked_process_quiet_deadline_for_layout(&pane_tracked_processes, &layout)?;

        pretty_assertions::assert_eq!(timers.tracked_process_quiet_deadline, None);
        assert2::assert!(timers.tracked_process_quiet_sleep.deadline() > focused_deadline);
        Ok(())
    }

    #[tokio::test(start_paused = true)]
    async fn test_client_timers_when_quiet_sleep_is_unpolled_reports_passed_deadline() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let config = crate::server::test_helpers::server_config(tempdir.path(), "work")?;
        let mut timers = ClientTimers::new(&config)?;
        timers
            .tracked_process_quiet_sleep
            .as_mut()
            .reset(tokio::time::Instant::now() + Duration::from_millis(1));

        assert2::assert!(!timers.tracked_process_quiet_sleep_deadline_has_passed());
        tokio::time::advance(Duration::from_millis(1)).await;

        assert2::assert!(timers.tracked_process_quiet_sleep_deadline_has_passed());
        Ok(())
    }

    fn layout(config: &crate::server::ServerConfig) -> rootcause::Result<SessionLayout> {
        let mut layout = SessionLayout::initial(&config.session, self::metadata("sh", 1))?;
        layout.split_active_pane(
            config.user_config.layout,
            self::metadata("sh", 2),
            crate::pane::split::PaneSplitAxis::Vertical,
        )?;
        Ok(layout)
    }

    fn metadata(cmd_label: &str, started_at: u64) -> crate::state::SessionMetadata {
        crate::state::SessionMetadata {
            cmd_label: cmd_label.to_owned(),
            cwd: "/tmp".to_owned(),
            started_at,
        }
    }

    fn fg_tracked_process(executable: &str) -> crate::pane::cmd::PaneCmdObservation {
        crate::pane::cmd::PaneCmdObservation::FgCmd(crate::pane::cmd::FgCmd::from_test_cmd(crate::pane::cmd::PaneCmd {
            executable: executable.to_owned(),
            path: None,
            pid: 42,
        }))
    }

    fn successful_exit_status() -> crate::pty::PtyExitStatus {
        crate::pty::PtyExitStatus {
            code: 0,
            signal: None,
            success: true,
        }
    }

    fn instant_after(instant: Instant, duration: Duration) -> rootcause::Result<Instant> {
        instant
            .checked_add(duration)
            .ok_or_else(|| rootcause::report!("test instant overflowed"))
    }

    fn tracked_process_quiet_sleep_deadline(
        instant: Instant,
        logical_delay: Duration,
    ) -> rootcause::Result<tokio::time::Instant> {
        let delay = logical_delay
            .checked_add(TRACKED_PROCESS_QUIET_SETTLE_DELAY)
            .ok_or_else(|| rootcause::report!("test quiet settle delay overflowed"))?;
        Ok(tokio::time::Instant::from_std(self::instant_after(instant, delay)?))
    }
}
