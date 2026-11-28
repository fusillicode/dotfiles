use std::iter;

use muxr_config::ScreenObservationConfig;
use muxr_config::TrackedProcess;
use muxr_config::TrackedProcessId;

use crate::terminal::TerminalScreenMode;

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
    ScreenOutput,
}

#[derive(Debug)]
pub(super) struct ScreenObserver {
    primary_interruptions: usize,
    alternate_interruptions: usize,
}

impl ScreenObserver {
    pub(super) const fn new() -> Self {
        Self {
            primary_interruptions: 0,
            alternate_interruptions: 0,
        }
    }

    pub(super) fn observe<'a>(
        &mut self,
        patterns: &ScreenObservationConfig,
        spans: impl DoubleEndedIterator<Item = impl Iterator<Item = &'a str>>,
        busy_start: BusyStart,
        mode: TerminalScreenMode,
    ) -> ScreenObservation<'a> {
        let literal = cancellation_literal(patterns);
        let mut lowest = ScreenObservation::Unknown;
        let mut interruptions = 0_usize;
        for mut span in spans.rev() {
            let Some(line) = span.next() else { continue };
            if matches_cancellation(patterns, literal, line, span) {
                interruptions = interruptions.saturating_add(1);
                if matches!(lowest, ScreenObservation::Unknown) {
                    lowest = ScreenObservation::Cancelled;
                }
            } else if matches!(lowest, ScreenObservation::Unknown) {
                let status = patterns.normalize_line(line);
                if patterns.matches_busy(status) {
                    lowest = ScreenObservation::Busy;
                } else if busy_start == BusyStart::Activity && patterns.matches_needs_attention(status) {
                    lowest = ScreenObservation::NeedsAttention(status);
                }
            }
            if busy_start == BusyStart::Activity && !matches!(lowest, ScreenObservation::Unknown) {
                return lowest;
            }
        }

        if busy_start == BusyStart::ScreenOutput {
            let previous = match mode {
                TerminalScreenMode::Normal => &mut self.primary_interruptions,
                TerminalScreenMode::Alternate => &mut self.alternate_interruptions,
            };
            // A banner can persist across replies. Counting only the bottom 12 rows keeps sampling cheap.
            // A resize or redraw can bring an old banner back into the tail and briefly clear the green dot.
            // The next reply update outside the input-echo window restores it.
            let new_interruption = interruptions > *previous;
            *previous = interruptions;
            if matches!(lowest, ScreenObservation::Cancelled) && !new_interruption {
                return ScreenObservation::Unknown;
            }
        }
        lowest
    }
}

pub(super) const fn busy_start(process: &TrackedProcess) -> BusyStart {
    match (&process.screen_observation, process.id) {
        (Some(_), TrackedProcessId::Codex) => BusyStart::ScreenOutput,
        (None, _)
        | (
            Some(_),
            TrackedProcessId::Claude | TrackedProcessId::Cursor | TrackedProcessId::Gemini | TrackedProcessId::Opencode,
        ) => BusyStart::Activity,
    }
}

#[cfg(test)]
fn text_line_spans(text: &str) -> impl DoubleEndedIterator<Item = impl Iterator<Item = &str>> {
    (0..text.lines().count()).map(move |index| text.lines().skip(index))
}

/// Only specialize an anchored, case-sensitive literal after the interruption glyph.
fn cancellation_literal(patterns: &ScreenObservationConfig) -> Option<&str> {
    let [pattern] = patterns.cancelled.as_ref()?.as_ref().as_slice() else {
        return None;
    };
    let literal = pattern.as_str().strip_prefix(r"(?u-ix)\A■\s+")?.strip_suffix(r"\z")?;
    let is_literal = literal
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b' ' | b'-' | b'/'));
    (is_literal && !literal.is_empty() && !literal.starts_with(' ')).then_some(literal)
}

fn matches_wrapped_literal<'a>(
    patterns: &ScreenObservationConfig,
    literal: &str,
    line: &'a str,
    continuations: impl Iterator<Item = &'a str>,
) -> bool {
    let Some(first) = patterns.normalize_line(line).strip_prefix('■') else {
        return false;
    };
    if !first.is_empty() && !first.starts_with(char::is_whitespace) {
        return false;
    }
    let first = first.trim_start();
    let Some(mut remaining) = literal.strip_prefix(first) else {
        return false;
    };
    let mut padded = !first.is_empty() && line.ends_with(char::is_whitespace);
    for row in continuations.take(SCREEN_TAIL_ROWS - 1) {
        if remaining.is_empty() {
            return true;
        }
        let fragment = patterns.normalize_line(row);
        if fragment.is_empty() {
            return false;
        }
        // A hard wrap splits at a space or inside a word. The literal determines which join can match.
        if let Some(rest) = remaining.strip_prefix(' ') {
            remaining = rest;
        } else if padded {
            return false;
        }
        let Some(rest) = remaining.strip_prefix(fragment) else {
            return false;
        };
        remaining = rest;
        padded = row.ends_with(char::is_whitespace);
    }
    remaining.is_empty()
}

fn matches_cancellation<'a>(
    patterns: &ScreenObservationConfig,
    literal: Option<&str>,
    line: &'a str,
    continuations: impl Iterator<Item = &'a str>,
) -> bool {
    if let Some(literal) = literal {
        return matches_wrapped_literal(patterns, literal, line, continuations);
    }
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
    let rows: Vec<_> = iter::once(line).chain(continuations).take(SCREEN_TAIL_ROWS).collect();
    let mut candidate = status.to_owned();
    matches_continuations(patterns, &mut candidate, &rows)
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
    let spaced_match = patterns.matches_cancelled(candidate) || matches_continuations(patterns, candidate, remaining);
    candidate.truncate(original_length);
    if spaced_match {
        return true;
    }
    if previous.ends_with(char::is_whitespace) {
        // Padding rules out a margin-induced word split.
        return false;
    }
    candidate.push_str(continuation);
    let adjacent_match = patterns.matches_cancelled(candidate) || matches_continuations(patterns, candidate, remaining);
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

    #[test]
    fn test_observer_when_working_hides_an_old_interruption_recognizes_only_a_new_interruption() {
        let patterns = patterns(TrackedProcessId::Codex).unwrap();
        let mut observer = ScreenObserver::new();
        let mut sample = |text: &str| {
            matches!(
                observer.observe(
                    &patterns,
                    text_line_spans(text),
                    BusyStart::ScreenOutput,
                    TerminalScreenMode::Normal
                ),
                ScreenObservation::Cancelled
            )
        };
        let interrupted = "■ Conversation interrupted - use /feedback if something went wrong";
        let working = format!("{interrupted}\nWorking (1s • esc to interrupt)");

        assert!(!sample(&working));
        assert!(!sample(interrupted));
        assert!(!sample("reply"));
        assert!(sample(interrupted));
    }

    #[rstest::rstest]
    #[case::insert_row(false)]
    #[case::rewrite_frame(true)]
    fn test_observer_when_banner_moves_or_returns_keeps_work_until_an_additional_interruption(
        #[case] rewrite: bool,
    ) -> rootcause::Result<()> {
        let size = TerminalSize::new(80, 12)?;
        let mut terminal = TerminalState::with_scrollback(&size, MuxrConfig::new()?.scrollback);
        let patterns = patterns(TrackedProcessId::Codex).ok_or_else(|| rootcause::report!("missing Codex patterns"))?;
        let mut observer = ScreenObserver::new();
        let interrupted = "■ Conversation interrupted - use /feedback if something went wrong";
        let observe = |observer: &mut ScreenObserver, terminal: &TerminalState| {
            let tail = terminal.live_tail_text(SCREEN_TAIL_ROWS);
            matches!(
                observer.observe(
                    &patterns,
                    tail.candidate_line_spans(),
                    BusyStart::ScreenOutput,
                    tail.screen_mode
                ),
                ScreenObservation::Cancelled
            )
        };
        let _ = terminal.process(format!("one\r\ntwo\r\n{interrupted}\r\nreply").as_bytes());
        test_that::assert_that!(observe(&mut observer, &terminal), eq(true));

        if rewrite {
            let _ = terminal.process(
                format!("\x1b[2;1H\x1b[2Kmore\r\n\x1b[2Ktwo\r\n\x1b[2K{interrupted}\r\n\x1b[2Kreply\x1b[2;5H")
                    .as_bytes(),
            );
        } else {
            let _ = terminal.process(b"\x1b[2;1H\x1b[Lmore");
        }
        test_that::assert_that!(observe(&mut observer, &terminal), eq(false));
        let _ = terminal.process(b"\x1b[?1049hviewer");
        test_that::assert_that!(observe(&mut observer, &terminal), eq(false));
        let _ = terminal.process(b"\x1b[?1049l");
        test_that::assert_that!(observe(&mut observer, &terminal), eq(false));

        let _ = terminal.process(format!("\x1b[7;1H{interrupted}").as_bytes());
        test_that::assert_that!(observe(&mut observer, &terminal), eq(true));
        Ok(())
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
            needs_attention: Some(ObservationPatterns::try_new(vec![
                Regex::clone(lazy_regex::regex!(r"\ADONE\b")),
                Regex::clone(lazy_regex::regex!(r"\AFINISHED\b")),
            ])?),
            cancelled: None,
            trim_chars: &['#'],
        };
        test_that::assert_that!(
            ScreenObserver::new().observe(
                &patterns,
                text_line_spans(text),
                BusyStart::Activity,
                TerminalScreenMode::Normal
            ),
            eq(expected)
        );
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
    #[case("Worked for 22m 38s • 11:37", ScreenObservation::Unknown)]
    #[case(
        "Working (6m 35s • ctrl+x to interrupt)\nWorked for 22m 38s • 11:37",
        ScreenObservation::Busy
    )]
    #[case(
        "Worked for 22m 38s • 11:37\nWorking (6m 35s • ctrl+x to interrupt)",
        ScreenObservation::Busy
    )]
    #[case("Worked for 22m 38s • 11:37\n\n› next prompt", ScreenObservation::Unknown)]
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
        ScreenObservation::Cancelled
    )]
    fn test_observe_when_status_rows_coexist_uses_last_recognized_row(
        #[case] text: &str,
        #[case] expected: ScreenObservation<'_>,
    ) {
        let patterns = patterns(TrackedProcessId::Codex).unwrap();
        assert_eq!(
            ScreenObserver::new().observe(
                &patterns,
                text_line_spans(text),
                BusyStart::ScreenOutput,
                TerminalScreenMode::Normal
            ),
            expected
        );
    }

    #[rstest::rstest]
    #[case(80, true, "Worked for 16m 15s • 14:18", ScreenObservation::Unknown)]
    #[case(80, true, "Working (6m 35s • ctrl+x to interrupt)", ScreenObservation::Busy)]
    #[case(20, false, "Worked for 16m 15s • 14:18", ScreenObservation::Unknown)]
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
            ScreenObserver::new().observe(
                &patterns,
                terminal.live_tail_text(SCREEN_TAIL_ROWS).candidate_line_spans(),
                BusyStart::ScreenOutput,
                TerminalScreenMode::Normal,
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
            ScreenObserver::new().observe(
                &patterns,
                terminal.live_tail_text(SCREEN_TAIL_ROWS).candidate_line_spans(),
                BusyStart::ScreenOutput,
                TerminalScreenMode::Normal,
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
            ScreenObserver::new().observe(
                &patterns,
                terminal.live_tail_text(SCREEN_TAIL_ROWS).candidate_line_spans(),
                BusyStart::ScreenOutput,
                TerminalScreenMode::Normal,
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
            r"\A■\s+STOP\z|\ACANCEL(?:LED|ED)\z"
        ))])?);
        test_that::assert_that!(
            ScreenObserver::new().observe(
                &patterns,
                terminal.live_tail_text(SCREEN_TAIL_ROWS).candidate_line_spans(),
                BusyStart::ScreenOutput,
                TerminalScreenMode::Normal,
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
            ScreenObserver::new().observe(
                &patterns,
                terminal.live_tail_text(SCREEN_TAIL_ROWS).candidate_line_spans(),
                BusyStart::ScreenOutput,
                TerminalScreenMode::Normal,
            ),
            eq(expected)
        );
        Ok(())
    }

    #[rstest::rstest]
    #[case("ordinary output")]
    #[case("■ unrelated error")]
    fn test_observe_when_working_follows_full_width_output_skips_cancellation_continuations(#[case] first_row: &str) {
        let patterns = patterns(TrackedProcessId::Codex).unwrap();
        let joined_rows = std::cell::Cell::new(false);
        let mut rows = ["ordinary output"; SCREEN_TAIL_ROWS];
        rows[0] = first_row;
        rows[SCREEN_TAIL_ROWS - 1] = "Working (1s • esc to interrupt)";
        let spans = rows.iter().enumerate().map(|(index, &line)| {
            let continuations = rows
                .iter()
                .skip(index.saturating_add(1))
                .copied()
                .inspect(|_| joined_rows.set(true));
            iter::once(line).chain(continuations)
        });
        assert_eq!(
            ScreenObserver::new().observe(&patterns, spans, BusyStart::ScreenOutput, TerminalScreenMode::Normal),
            ScreenObservation::Busy
        );
        assert!(!joined_rows.get());
    }

    #[test]
    fn test_observe_when_cancellation_is_disabled_ignores_hard_wrapped_message() {
        let mut patterns = patterns(TrackedProcessId::Codex).unwrap();
        patterns.cancelled = None;
        let text = "■ Conversation interrupted - use\n/feedback if something went wrong";
        assert_eq!(
            ScreenObserver::new().observe(
                &patterns,
                text_line_spans(text),
                BusyStart::ScreenOutput,
                TerminalScreenMode::Normal
            ),
            ScreenObservation::Unknown
        );
    }

    fn patterns(agent: TrackedProcessId) -> Option<ScreenObservationConfig> {
        MuxrConfig::new()
            .unwrap()
            .tracked_processes
            .processes
            .into_iter()
            .find(|process| process.id == agent)
            .and_then(|process| process.screen_observation)
    }
}
