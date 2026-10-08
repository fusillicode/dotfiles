use std::iter::once;

use muxr_config::ScreenObservationConfig;
use muxr_config::TrackedProcess;
use muxr_config::TrackedProcessId;

pub(super) const SCREEN_TAIL_ROWS: usize = 12;

#[derive(Clone, Copy)]
#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
pub(super) enum ScreenObservation<'a> {
    Busy,
    Cancelled,
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
    spans: impl DoubleEndedIterator<Item = impl Iterator<Item = &'a str>>,
) -> ScreenObservation<'a> {
    // Status rows from earlier turns can remain visible. The lowest recognized row takes precedence.
    for mut span in spans.rev() {
        let Some(line) = span.next() else {
            continue;
        };
        if self::matches_cancellation(patterns, line, span) {
            return ScreenObservation::Cancelled;
        }
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
pub(super) fn text_line_spans(text: &str) -> impl DoubleEndedIterator<Item = impl Iterator<Item = &str>> {
    (0..text.lines().count()).map(move |index| text.lines().skip(index))
}

fn matches_cancellation<'a>(
    patterns: &ScreenObservationConfig,
    line: &'a str,
    continuations: impl Iterator<Item = &'a str>,
) -> bool {
    if patterns.cancelled.is_none() {
        return false;
    }
    let status = patterns.normalize_line(line);
    if patterns.matches_cancelled(status) {
        return true;
    }
    if status.is_empty() {
        return false;
    }
    let mut continuations = continuations
        .take_while(|line| !patterns.normalize_line(line).is_empty())
        .peekable();
    if continuations.peek().is_none() {
        return false;
    }
    // Preserve padding so full-width hard rows can be joined within a word as well as between words. Limit the
    // ambiguous search to the same 12-row window and reuse one buffer; completion strings remain unchanged.
    let rows: Vec<_> = once(line).chain(continuations).take(SCREEN_TAIL_ROWS).collect();
    let mut candidate = status.to_owned();
    self::matches_continuations(patterns, &mut candidate, &rows)
}

fn matches_continuations(patterns: &ScreenObservationConfig, candidate: &mut String, rows: &[&str]) -> bool {
    let Some((previous, remaining)) = rows.split_first() else {
        return false;
    };
    let Some((continuation, _)) = remaining.split_first() else {
        return false;
    };
    let continuation = patterns.normalize_line(continuation);
    let original_length = candidate.len();
    candidate.push(' ');
    candidate.push_str(continuation);
    let spaced_match =
        patterns.matches_cancelled(candidate) || self::matches_continuations(patterns, candidate, remaining);
    candidate.truncate(original_length);
    if spaced_match {
        return true;
    }
    if previous.ends_with(char::is_whitespace) {
        // Padding rules out a margin-induced word split.
        return false;
    }
    candidate.push_str(continuation);
    let adjacent_match =
        patterns.matches_cancelled(candidate) || self::matches_continuations(patterns, candidate, remaining);
    candidate.truncate(original_length);
    adjacent_match
}

#[cfg(test)]
mod tests {
    use lazy_regex::Regex;
    use muxr_config::MuxrConfig;
    use muxr_config::ObservationPatterns;
    use muxr_core::TerminalSize;
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
                Regex::clone(lazy_regex::regex!(r"\ARUN(?:NING)?\z")),
                Regex::clone(lazy_regex::regex!(r"\ATHINK(?:ING)?\z")),
            ])?,
            needs_attention: ObservationPatterns::try_new(vec![
                Regex::clone(lazy_regex::regex!(r"\ADONE\b")),
                Regex::clone(lazy_regex::regex!(r"\AFINISHED\b")),
            ])?,
            cancelled: None,
            trim_chars: &['#'],
        };
        test_that::assert_that!(observe(&patterns, text_line_spans(text)), eq(expected));
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
    #[case(
        "■ Conversation interrupted - use /feedback if something went wrong",
        ScreenObservation::Cancelled
    )]
    #[case(
        "Working (1s • esc to interrupt)\n■ Conversation interrupted - use /feedback if something went wrong",
        ScreenObservation::Cancelled
    )]
    #[case(
        "■ Conversation interrupted - use /feedback if something went wrong\nWorking (1s • esc to interrupt)",
        ScreenObservation::Busy
    )]
    #[case(
        "■ Conversation interrupted - use\n/feedback if something went wrong",
        ScreenObservation::Cancelled
    )]
    #[case(
        "■ Conversation interrupted - use\n\n/feedback if something went wrong",
        ScreenObservation::Unknown
    )]
    #[case(
        "› ■ Conversation interrupted - use\n/feedback if something went wrong",
        ScreenObservation::Unknown
    )]
    #[case(
        "■ Conversation interrupted - use\n/feedback if something went wrong\nWorking (1s • esc to interrupt)",
        ScreenObservation::Busy
    )]
    #[case(
        "■ Conversation interrupted - use\n/feedback if something went wrong\nWorked for 1s • 14:18",
        ScreenObservation::NeedsAttention("Worked for 1s • 14:18")
    )]
    fn test_observe_when_status_rows_coexist_uses_last_recognized_row(
        #[case] text: &str,
        #[case] expected: ScreenObservation<'_>,
    ) {
        let patterns = patterns(TrackedProcessId::Codex).unwrap();
        assert_eq!(observe(&patterns, text_line_spans(text)), expected);
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
    #[case(
        80,
        true,
        "■ Conversation interrupted - use /feedback if something went wrong",
        ScreenObservation::Cancelled
    )]
    #[case(
        20,
        false,
        "■ Conversation interrupted - use /feedback if something went wrong",
        ScreenObservation::Cancelled
    )]
    fn test_observe_when_status_is_cursor_painted_or_wrapped_recognizes_status(
        #[case] columns: u16,
        #[case] stale_wrap: bool,
        #[case] status: &str,
        #[case] expected: ScreenObservation<'_>,
    ) -> rootcause::Result<()> {
        let mut terminal =
            TerminalState::with_scrollback(&TerminalSize::new(columns, 5)?, MuxrConfig::new()?.scrollback);
        if stale_wrap {
            let _output = terminal.process(format!("{}\x1b[2;1H", "x".repeat(81)).as_bytes());
        }
        let _output = terminal.process(format!("  {status}").as_bytes());
        let patterns = patterns(TrackedProcessId::Codex).ok_or_else(|| rootcause::report!("missing Codex patterns"))?;
        test_that::assert_that!(
            observe(
                &patterns,
                terminal.live_tail_text(SCREEN_TAIL_ROWS).candidate_line_spans()
            ),
            eq(expected)
        );
        Ok(())
    }

    #[rstest::rstest]
    #[case("■ Conversation interrupted - use\r\n/feedback if something went wrong")]
    #[case("\x1b[1;1H■ Conversation interrupted - use\x1b[2;1H/feedback if something went wrong")]
    #[case("■ Conversation interrupted - use /feedback\r\nif something went wrong")]
    fn test_observe_when_cancellation_is_hard_wrapped_recognizes_status(#[case] output: &str) -> rootcause::Result<()> {
        let mut terminal = TerminalState::with_scrollback(&TerminalSize::new(40, 5)?, MuxrConfig::new()?.scrollback);
        let _output = terminal.process(output.as_bytes());
        let patterns = patterns(TrackedProcessId::Codex).ok_or_else(|| rootcause::report!("missing Codex patterns"))?;
        test_that::assert_that!(
            observe(
                &patterns,
                terminal.live_tail_text(SCREEN_TAIL_ROWS).candidate_line_spans()
            ),
            eq(ScreenObservation::Cancelled)
        );
        Ok(())
    }

    #[rstest::rstest]
    #[case(
        10,
        "■\r\nConversati\r\non\r\ninterrupte\r\nd - use\r\n/feedback\r\nif\r\nsomething\r\nwent wrong"
    )]
    #[case(
        11,
        "■\r\nConversatio\r\nn\r\ninterrupted\r\n- use\r\n/feedback\r\nif\r\nsomething\r\nwent wrong"
    )]
    #[case(
        8,
        "■\r\nConversa\r\ntion\r\ninterrup\r\nted -\r\nuse\r\n/feedbac\r\nk if\r\nsomethin\r\ng went\r\nwrong"
    )]
    fn test_observe_when_hard_wrap_splits_words_recognizes_cancellation(
        #[case] columns: u16,
        #[case] output: &str,
    ) -> rootcause::Result<()> {
        let mut terminal =
            TerminalState::with_scrollback(&TerminalSize::new(columns, 12)?, MuxrConfig::new()?.scrollback);
        let _output = terminal.process(output.as_bytes());
        let patterns = patterns(TrackedProcessId::Codex).ok_or_else(|| rootcause::report!("missing Codex patterns"))?;
        test_that::assert_that!(
            observe(
                &patterns,
                terminal.live_tail_text(SCREEN_TAIL_ROWS).candidate_line_spans()
            ),
            eq(ScreenObservation::Cancelled)
        );
        Ok(())
    }

    #[rstest::rstest]
    #[case("CANC\r\nELLE\r\nD", ScreenObservation::Cancelled)]
    #[case("CANC\r\nELED", ScreenObservation::Cancelled)]
    #[case("CANC\r\nELLE\r\nDX", ScreenObservation::Unknown)]
    fn test_observe_when_custom_cancellation_splits_words_preserves_regex_semantics(
        #[case] output: &str,
        #[case] expected: ScreenObservation<'_>,
    ) -> rootcause::Result<()> {
        let mut terminal = TerminalState::with_scrollback(&TerminalSize::new(4, 4)?, MuxrConfig::new()?.scrollback);
        let _output = terminal.process(output.as_bytes());
        let mut patterns =
            patterns(TrackedProcessId::Codex).ok_or_else(|| rootcause::report!("missing Codex patterns"))?;
        patterns.cancelled = Some(ObservationPatterns::try_new(vec![Regex::clone(lazy_regex::regex!(
            r"\ACANCEL(?:LED|ED)\z"
        ))])?);
        test_that::assert_that!(
            observe(
                &patterns,
                terminal.live_tail_text(SCREEN_TAIL_ROWS).candidate_line_spans()
            ),
            eq(expected)
        );
        Ok(())
    }

    #[rstest::rstest]
    #[case(1, false, ScreenObservation::Unknown)]
    #[case(2, false, ScreenObservation::Cancelled)]
    #[case(1, true, ScreenObservation::Unknown)]
    #[case(2, true, ScreenObservation::Cancelled)]
    fn test_observe_when_cancellation_is_at_tail_boundary_reads_only_last_twelve_rows(
        #[case] row: u16,
        #[case] hard_wrapped: bool,
        #[case] expected: ScreenObservation<'_>,
    ) -> rootcause::Result<()> {
        let mut terminal = TerminalState::with_scrollback(&TerminalSize::new(80, 13)?, MuxrConfig::new()?.scrollback);
        let message = if hard_wrapped {
            "■ Conversation interrupted - use\r\n/feedback if something went wrong"
        } else {
            "■ Conversation interrupted - use /feedback if something went wrong"
        };
        let _output = terminal.process(format!("\x1b[{row};1H{message}").as_bytes());
        let patterns = patterns(TrackedProcessId::Codex).ok_or_else(|| rootcause::report!("missing Codex patterns"))?;
        test_that::assert_that!(
            observe(
                &patterns,
                terminal.live_tail_text(SCREEN_TAIL_ROWS).candidate_line_spans()
            ),
            eq(expected)
        );
        Ok(())
    }

    #[test]
    fn test_observe_when_cancellation_is_disabled_ignores_hard_wrapped_message() {
        let mut patterns = patterns(TrackedProcessId::Codex).unwrap();
        patterns.cancelled = None;
        let text = "■ Conversation interrupted - use\n/feedback if something went wrong";
        assert_eq!(observe(&patterns, text_line_spans(text)), ScreenObservation::Unknown);
    }
}
