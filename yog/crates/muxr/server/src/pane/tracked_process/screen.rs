use muxr_config::TrackedProcessId;

mod codex;

pub(super) const SCREEN_TAIL_ROWS: usize = 12;

type ScreenPattern = fn(&str) -> bool;
type AttentionPattern = fn(&str) -> Option<&str>;

#[derive(Clone, Copy)]
#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
pub(super) enum ScreenObservation<'a> {
    Busy,
    NeedsAttention(&'a str),
    Unknown,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum BusyStart {
    Activity,
    Screen,
}

pub(super) struct ScreenPatterns {
    pub(super) busy_start: BusyStart,
    busy: &'static [ScreenPattern],
    attention: &'static [AttentionPattern],
}

impl ScreenPatterns {
    pub(super) fn observe<'a>(&self, lines: impl DoubleEndedIterator<Item = &'a str>) -> ScreenObservation<'a> {
        // Status rows from earlier turns can remain visible. The lowest recognized row takes precedence.
        for line in lines.rev() {
            if self.busy.iter().any(|pattern| pattern(line)) {
                return ScreenObservation::Busy;
            }
            if let Some(completion) = self.attention.iter().find_map(|pattern| pattern(line)) {
                return ScreenObservation::NeedsAttention(completion);
            }
        }
        // An absent busy row alone is not confirmation that the agent finished.
        ScreenObservation::Unknown
    }
}

/// Register both checks together; attention patterns return the status text used to reject pre-submission footers.
pub(super) const fn patterns(process: TrackedProcessId) -> Option<ScreenPatterns> {
    match process {
        TrackedProcessId::Codex => Some(ScreenPatterns {
            busy_start: BusyStart::Screen,
            busy: &[codex::busy],
            attention: &[codex::needs_attention],
        }),
        TrackedProcessId::Claude | TrackedProcessId::Cursor | TrackedProcessId::Gemini | TrackedProcessId::Opencode => {
            None
        }
    }
}

pub(super) const fn busy_start(process: TrackedProcessId) -> BusyStart {
    match patterns(process) {
        Some(patterns) => patterns.busy_start,
        None => BusyStart::Activity,
    }
}

#[cfg(test)]
mod tests {
    use muxr_config::MuxrConfig;
    use muxr_core::TerminalSize;
    use test_that::prelude::*;

    use super::*;
    use crate::terminal::TerminalState;

    #[rstest::rstest]
    #[case(TrackedProcessId::Claude)]
    #[case(TrackedProcessId::Cursor)]
    #[case(TrackedProcessId::Gemini)]
    #[case(TrackedProcessId::Opencode)]
    fn test_patterns_when_agent_has_no_screen_checks_returns_none(#[case] agent: TrackedProcessId) {
        assert!(patterns(agent).is_none());
    }

    #[rstest::rstest]
    #[case("Working (6m 35s • ctrl+x to interrupt)", ScreenObservation::Busy)]
    #[case(
        "Worked for 22m 38s • 11:37",
        ScreenObservation::NeedsAttention("Worked for 22m 38s • 11:37")
    )]
    #[case(
        "Working (6m 35s • ctrl+x to interrupt)\nWorked for 22m 38s • 11:37",
        ScreenObservation::NeedsAttention("Worked for 22m 38s • 11:37")
    )]
    #[case(
        "Worked for 22m 38s • 11:37\nWorking (6m 35s • ctrl+x to interrupt)",
        ScreenObservation::Busy
    )]
    #[case(
        "Worked for 22m 38s • 11:37\n\n› next prompt",
        ScreenObservation::NeedsAttention("Worked for 22m 38s • 11:37")
    )]
    #[case("no recognized status", ScreenObservation::Unknown)]
    fn test_observe_when_status_rows_coexist_uses_last_recognized_row(
        #[case] text: &str,
        #[case] expected: ScreenObservation<'_>,
    ) {
        let patterns = patterns(TrackedProcessId::Codex).unwrap();
        assert_eq!(patterns.observe(text.lines()), expected);
    }

    #[rstest::rstest]
    #[case(
        80,
        true,
        "Worked for 16m 15s • 14:18",
        ScreenObservation::NeedsAttention("Worked for 16m 15s • 14:18")
    )]
    #[case(80, true, "Working (6m 35s • ctrl+x to interrupt)", ScreenObservation::Busy)]
    #[case(
        20,
        false,
        "Worked for 16m 15s • 14:18",
        ScreenObservation::NeedsAttention("Worked for 16m 15s • 14:18")
    )]
    #[case(20, false, "Working (6m 35s • ctrl+x to interrupt)", ScreenObservation::Busy)]
    fn test_observe_when_status_is_cursor_painted_or_wrapped_recognizes_status(
        #[case] columns: u16,
        #[case] stale_wrap: bool,
        #[case] status: &str,
        #[case] expected: ScreenObservation<'_>,
    ) -> rootcause::Result<()> {
        let mut terminal =
            TerminalState::with_scrollback(&TerminalSize::new(columns, 3)?, MuxrConfig::default().scrollback);
        if stale_wrap {
            let _output = terminal.process(format!("{}\x1b[2;1H", "x".repeat(81)).as_bytes());
        }
        let _output = terminal.process(format!("  {status}").as_bytes());
        let patterns = patterns(TrackedProcessId::Codex).ok_or_else(|| rootcause::report!("missing Codex patterns"))?;
        test_that::assert_that!(
            patterns.observe(terminal.live_tail_text(SCREEN_TAIL_ROWS).candidate_lines()),
            eq(expected)
        );
        Ok(())
    }
}
