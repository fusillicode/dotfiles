use std::collections::BTreeSet;
use std::pin::Pin;
use std::time::Duration;
use std::time::Instant;

use muxr_core::PaneId;
use rootcause::report;

use crate::server::ServerConfig;

const CMD_HANDOFF_SAMPLE_DELAY: Duration = Duration::from_millis(50);
const INTERACTIVE_RENDER_BOOST_FOR: Duration = Duration::from_millis(250);
const INTERACTIVE_RENDER_FRAME_INTERVAL: Duration = Duration::from_millis(10);
const RENDER_FRAME_INTERVAL: Duration = Duration::from_millis(16);
const SLEEP_DISABLED_FOR: Duration = Duration::from_hours(24);

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
    // Option 3 was a separate user-input deadline path; the adaptive cap keeps one scheduler and tags only recent
    // PTY-bound input as latency-sensitive.
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

    pub fn sync_tracked_process_quiet_deadline(&mut self, deadline: Option<Instant>) -> rootcause::Result<()> {
        if self.tracked_process_quiet_deadline == deadline {
            return Ok(());
        }

        self.tracked_process_quiet_deadline = deadline;
        let deadline = deadline.map_or_else(self::disabled_sleep_deadline, |deadline| {
            Ok(tokio::time::Instant::from_std(deadline))
        })?;
        self.tracked_process_quiet_sleep.as_mut().reset(deadline);
        Ok(())
    }

    pub fn disable_tracked_process_quiet_sleep(&mut self) -> rootcause::Result<()> {
        self.tracked_process_quiet_deadline = None;
        self.tracked_process_quiet_sleep
            .as_mut()
            .reset(self::disabled_sleep_deadline()?);
        Ok(())
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
}
