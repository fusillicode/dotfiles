use muxr_config::ScreenObservationConfig;
use muxr_config::TrackedProcess;
use muxr_config::TrackedProcessId;

pub(super) const SCREEN_TAIL_ROWS: usize = 12;

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

pub(super) fn observe<'a>(
    patterns: &ScreenObservationConfig,
    lines: impl DoubleEndedIterator<Item = &'a str>,
) -> ScreenObservation<'a> {
    // Status rows from earlier turns can remain visible. The lowest recognized row takes precedence.
    for line in lines.rev() {
        let status = patterns.normalize_line(line);
        if patterns.matches_busy(status) {
            return ScreenObservation::Busy;
        }
        if patterns.matches_needs_attention(status) {
            // Keep the full normalized status, even when a configured regex matches only part of it.
            return ScreenObservation::NeedsAttention(status);
        }
    }
    // An absent busy row alone is not confirmation that the agent finished.
    ScreenObservation::Unknown
}

pub(super) const fn busy_start(process: &TrackedProcess) -> BusyStart {
    match (&process.screen_observation, process.id) {
        (Some(_), TrackedProcessId::Codex) => BusyStart::Screen,
        (None, _)
        | (
            Some(_),
            TrackedProcessId::Claude | TrackedProcessId::Cursor | TrackedProcessId::Gemini | TrackedProcessId::Opencode,
        ) => BusyStart::Activity,
    }
}

#[cfg(test)]
mod tests {
    use muxr_config::MuxrConfig;
    use muxr_config::ObservationPatterns;
    use muxr_core::TerminalSize;
    use regex::Regex;
    use test_that::prelude::*;

    use super::*;
    use crate::terminal::TerminalState;

    fn patterns(agent: TrackedProcessId) -> Option<ScreenObservationConfig> {
        MuxrConfig::new()
            .unwrap()
            .tracked_processes
            .processes
            .into_iter()
            .find(|process| process.id == agent)
            .and_then(|process| process.screen_observation)
    }

    #[rstest::rstest]
    #[case("RUNNING", ScreenObservation::Busy)]
    #[case("THINKING", ScreenObservation::Busy)]
    #[case("DONE turn 12", ScreenObservation::NeedsAttention("DONE turn 12"))]
    #[case("FINISHED turn 13", ScreenObservation::NeedsAttention("FINISHED turn 13"))]
    #[case("RUNNING\nDONE turn 14", ScreenObservation::NeedsAttention("DONE turn 14"))]
    #[case("DONE turn 14\nTHINKING", ScreenObservation::Busy)]
    #[case(" # RUNNING # ", ScreenObservation::Busy)]
    #[case(" # DONE turn 15 # ", ScreenObservation::NeedsAttention("DONE turn 15"))]
    #[case("• RUNNING", ScreenObservation::Unknown)]
    #[case("Working (1s • esc to interrupt)", ScreenObservation::Unknown)]
    fn test_observe_when_patterns_are_configured_uses_all_alternatives_and_preserves_completion(
        #[case] text: &str,
        #[case] expected: ScreenObservation<'_>,
    ) -> rootcause::Result<()> {
        let patterns = ScreenObservationConfig {
            busy: ObservationPatterns::try_new(vec![
                Regex::new(r"\ARUN(?:NING)?\z")?,
                Regex::new(r"\ATHINK(?:ING)?\z")?,
            ])?,
            needs_attention: ObservationPatterns::try_new(vec![
                Regex::new(r"\ADONE\b")?,
                Regex::new(r"\AFINISHED\b")?,
            ])?,
            trim_chars: &['#'],
        };
        test_that::assert_that!(observe(&patterns, text.lines()), eq(expected));
        Ok(())
    }

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
        assert_eq!(observe(&patterns, text.lines()), expected);
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
            TerminalState::with_scrollback(&TerminalSize::new(columns, 3)?, MuxrConfig::new()?.scrollback);
        if stale_wrap {
            let _output = terminal.process(format!("{}\x1b[2;1H", "x".repeat(81)).as_bytes());
        }
        let _output = terminal.process(format!("  {status}").as_bytes());
        let patterns = patterns(TrackedProcessId::Codex).ok_or_else(|| rootcause::report!("missing Codex patterns"))?;
        test_that::assert_that!(
            observe(&patterns, terminal.live_tail_text(SCREEN_TAIL_ROWS).candidate_lines()),
            eq(expected)
        );
        Ok(())
    }
}
