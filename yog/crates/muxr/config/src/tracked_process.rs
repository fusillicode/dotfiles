//! Agent defaults, command matching, and screen recognition rules.

use std::time::Duration;

use nonempty_collections::NEVec;
use nonempty_collections::nev;
use regex::Regex;
use rootcause::prelude::ResultExt;

/// One foreground process class that can drive tab-bar dots and quiet-attention state.
#[derive(Clone, Debug)]
pub struct TrackedProcess {
    pub id: TrackedProcessId,
    pub label: &'static str,
    pub matchers: Vec<ProcessMatcher>,
    pub quiet_threshold: Duration,
    pub screen_observation: Option<ScreenObservationConfig>,
}

impl TrackedProcess {
    /// Return true when any matcher identifies this tracked process.
    pub fn matches(&self, executable: &str, path: Option<&str>) -> bool {
        self.matchers.iter().any(|matcher| matcher.matches(executable, path))
    }
}

/// Compiled regex alternatives checked against the last 12 live terminal rows.
///
/// The lowest recognized status takes priority. Patterns match normalized text: whitespace and configured
/// decoration characters are trimmed from both ends. Anchor patterns to avoid matching quoted status examples.
/// Edit the static values in `tracked_process.rs` and rebuild to tune them.
#[derive(Clone, Debug)]
pub struct ScreenObservationConfig {
    pub busy: NEVec<Regex>,
    pub needs_attention: NEVec<Regex>,
    /// Additional edge decorations to trim; whitespace is always trimmed.
    pub trim_chars: &'static [char],
}

impl ScreenObservationConfig {
    /// Trim whitespace and configured decorations without changing the status text between them.
    pub fn normalize_line<'a>(&self, line: &'a str) -> &'a str {
        line.trim_matches(|character: char| character.is_whitespace() || self.trim_chars.contains(&character))
    }

    /// Match a normalized status against the configured busy patterns.
    pub fn matches_busy(&self, status: &str) -> bool {
        self.busy.iter().any(|pattern| pattern.is_match(status))
    }

    /// Match a normalized status against the configured attention patterns.
    pub fn matches_needs_attention(&self, status: &str) -> bool {
        self.needs_attention.iter().any(|pattern| pattern.is_match(status))
    }
}

/// Stable ids for initially configured tracked processes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrackedProcessId {
    Claude,
    Codex,
    Cursor,
    Gemini,
    Opencode,
}

/// A foreground process matcher.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProcessMatcher {
    ExactExecutable(&'static str),
    PathContains(&'static str),
    ExecutableWithPathContains {
        executable: &'static str,
        path_contains: &'static str,
    },
}

impl ProcessMatcher {
    /// Return true when this matcher identifies a foreground process.
    pub fn matches(self, executable: &str, path: Option<&str>) -> bool {
        match self {
            Self::ExactExecutable(expected) => executable == expected,
            Self::PathContains(needle) => path.is_some_and(|path| path.contains(needle)),
            Self::ExecutableWithPathContains {
                executable: expected,
                path_contains,
            } => executable == expected && path.is_some_and(|path| path.contains(path_contains)),
        }
    }
}

/// Build the configured agents and compile their screen patterns.
///
/// # Errors
/// Returns an error if a configured screen regex is invalid.
pub fn defaults() -> rootcause::Result<Vec<TrackedProcess>> {
    Ok(vec![
        TrackedProcess {
            id: TrackedProcessId::Claude,
            label: "cl",
            matchers: vec![
                ProcessMatcher::ExactExecutable("claude"),
                ProcessMatcher::ExactExecutable("claude-code"),
                ProcessMatcher::PathContains("/claude/versions/"),
            ],
            quiet_threshold: Duration::from_secs(3),
            screen_observation: None,
        },
        TrackedProcess {
            id: TrackedProcessId::Codex,
            label: "cx",
            matchers: vec![
                ProcessMatcher::ExactExecutable("codex"),
                ProcessMatcher::ExactExecutable("codex-aarch64-apple-darwin"),
                ProcessMatcher::ExactExecutable("codex-x86_64-apple-darwin"),
            ],
            quiet_threshold: Duration::from_secs(3),
            screen_observation: Some(ScreenObservationConfig {
                busy: nev![
                    Regex::new(r"\AWorking \(\s*(?:[0-9]+h(?:\s+[0-9]+m)?(?:\s+[0-9]+s)?|[0-9]+m(?:\s+[0-9]+s)?|[0-9]+s)\s*•\s*\S+ to interrupt\s*\)\z").context("invalid Codex busy regex (index=0)")?,
                ],
                needs_attention: nev![
                    Regex::new(r"\AWorked for \s*(?:[0-9]+h(?:\s+[0-9]+m)?(?:\s+[0-9]+s)?|[0-9]+m(?:\s+[0-9]+s)?|[0-9]+s)\s*•\s*(?:[01][0-9]|2[0-3]):[0-5][0-9]\z").context("invalid Codex needs_attention regex (index=0)")?,
                ],
                trim_chars: &['─', '━', '•', '·', '⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'],
            }),
        },
        TrackedProcess {
            id: TrackedProcessId::Cursor,
            label: "cu",
            matchers: vec![
                ProcessMatcher::ExactExecutable("cursor"),
                ProcessMatcher::ExactExecutable("cursor-agent"),
                ProcessMatcher::ExecutableWithPathContains {
                    executable: "node",
                    path_contains: "/cursor-agent/versions/",
                },
            ],
            quiet_threshold: Duration::from_secs(3),
            screen_observation: None,
        },
        TrackedProcess {
            id: TrackedProcessId::Gemini,
            label: "gm",
            matchers: vec![ProcessMatcher::ExactExecutable("gemini")],
            quiet_threshold: Duration::from_secs(3),
            screen_observation: None,
        },
        TrackedProcess {
            id: TrackedProcessId::Opencode,
            label: "oc",
            matchers: vec![ProcessMatcher::ExactExecutable("opencode")],
            quiet_threshold: Duration::from_secs(3),
            screen_observation: None,
        },
    ])
}

#[cfg(test)]
mod tests {
    use regex::RegexBuilder;
    use test_that::prelude::*;

    use super::*;
    use crate::MuxrConfig;

    #[rstest::rstest]
    #[case(&[], " \t status \u{2003}", "status")]
    #[case(&[], " • status ─ ", "• status ─")]
    #[case(&['#'], " # status # ", "status")]
    #[case(&['#'], " # status # detail # ", "status # detail")]
    #[case(&['#'], " # • status ─ # ", "• status ─")]
    #[case(&['#'], " #\t# ", "")]
    #[case(&['#'], "", "")]
    fn test_screen_observation_when_normalizing_trims_only_configured_edges_and_whitespace(
        #[case] trim_chars: &'static [char],
        #[case] line: &str,
        #[case] expected: &str,
    ) -> rootcause::Result<()> {
        let screen = ScreenObservationConfig {
            busy: nonempty_collections::nev![Regex::new(r"\Astatus\s+active\z")?],
            needs_attention: nonempty_collections::nev![Regex::new(r"\Adone[0-9]+\z")?],
            trim_chars,
        };
        test_that::assert_that!(screen.normalize_line(line), eq(expected));
        Ok(())
    }

    #[test]
    fn test_screen_observation_when_regex_has_builder_options_preserves_compiled_behavior() -> rootcause::Result<()> {
        let screen = ScreenObservationConfig {
            busy: nonempty_collections::nev![RegexBuilder::new(r"\Awork(?:ing)?\z").case_insensitive(true).build()?],
            needs_attention: nonempty_collections::nev![Regex::new(r"\Adone[0-9]+\z")?],
            trim_chars: &[],
        };
        test_that::assert_that!(screen.matches_busy("WORKING"), eq(true));
        test_that::assert_that!(screen.matches_needs_attention("DONE1"), eq(false));
        test_that::assert_that!(screen.matches_needs_attention("done1"), eq(true));
        Ok(())
    }

    #[rstest::rstest]
    #[case("Working (6m 35s • ctrl+x to interrupt)")]
    #[case("• Working (0s • esc to interrupt)")]
    #[case("⠹ Working (1h 2m 3s • ctrl+c to interrupt) ")]
    #[case("Working (1h • esc to interrupt)")]
    #[case("Working (2m • ctrl+c to interrupt)")]
    #[case("Working ( 1h\t2m 3s \t•\tctrl+x to interrupt )")]
    fn test_codex_busy_when_status_is_valid_matches_pattern(#[case] text: &str) {
        let screen = codex_screen_observation();
        assert!(screen.matches_busy(screen.normalize_line(text)));
    }

    fn codex_screen_observation() -> ScreenObservationConfig {
        MuxrConfig::new()
            .unwrap()
            .tracked_processes
            .into_iter()
            .find(|process| process.id == TrackedProcessId::Codex)
            .unwrap()
            .screen_observation
            .unwrap()
    }

    #[rstest::rstest]
    #[case("Worked for 22m 38s • 11:37", "Worked for 22m 38s • 11:37")]
    #[case("── Worked for 0s • 00:01 ───", "Worked for 0s • 00:01")]
    #[case("Worked for 1h 2m 3s • 23:59", "Worked for 1h 2m 3s • 23:59")]
    #[case("\t── Worked for   22m\t38s  •\t11:37 ── ", "Worked for   22m\t38s  •\t11:37")]
    fn test_codex_needs_attention_when_completion_is_valid_matches_normalized_status(
        #[case] text: &str,
        #[case] expected: &str,
    ) {
        let screen = codex_screen_observation();
        let status = screen.normalize_line(text);
        assert_eq!(status, expected);
        assert!(screen.matches_needs_attention(status));
        assert!(!screen.matches_busy(status));
    }

    #[rstest::rstest]
    #[case("Working (6m 35s • ctrl+x to interrupt)")]
    #[case("› Worked for 22m 38s • 11:37")]
    #[case("example: Worked for 22m 38s • 11:37")]
    #[case("Worked for 1s • 24:00")]
    #[case("Worked for 1s • 12:60")]
    #[case("Worked for 1s • 12:30 quoted text")]
    #[case("Worked for • 12:30")]
    fn test_codex_needs_attention_when_text_is_not_a_completion_rejects_pattern(#[case] text: &str) {
        let screen = codex_screen_observation();
        assert!(!screen.matches_needs_attention(screen.normalize_line(text)));
    }

    #[rstest::rstest]
    #[case("› Working (6m 35s • ctrl+x to interrupt)")]
    #[case("example: Working (6m 35s • ctrl+x to interrupt)")]
    #[case("Working (later • ctrl+x to interrupt)")]
    #[case("Working (1s 2m • ctrl+x to interrupt)")]
    #[case("Working (1s • to interrupt)")]
    #[case("Worked for 22m 38s • 11:37")]
    fn test_codex_busy_when_text_is_not_a_working_status_rejects_pattern(#[case] text: &str) {
        let screen = codex_screen_observation();
        assert!(!screen.matches_busy(screen.normalize_line(text)));
    }

    #[rstest::rstest]
    #[case::claude("claude", None, TrackedProcessId::Claude, "cl")]
    #[case::claude_code("claude-code", None, TrackedProcessId::Claude, "cl")]
    #[case::claude_versioned_runtime(
        "node",
        Some("/Users/me/claude/versions/1.2.3/node"),
        TrackedProcessId::Claude,
        "cl"
    )]
    #[case::codex("codex", None, TrackedProcessId::Codex, "cx")]
    #[case::codex_aarch64("codex-aarch64-apple-darwin", None, TrackedProcessId::Codex, "cx")]
    #[case::cursor("cursor", None, TrackedProcessId::Cursor, "cu")]
    #[case::cursor_agent("cursor-agent", None, TrackedProcessId::Cursor, "cu")]
    #[case::cursor_versioned_runtime(
        "node",
        Some("/Users/me/cursor-agent/versions/1.2.3/node"),
        TrackedProcessId::Cursor,
        "cu"
    )]
    #[case::gemini("gemini", None, TrackedProcessId::Gemini, "gm")]
    #[case::opencode("opencode", None, TrackedProcessId::Opencode, "oc")]
    fn test_tracked_processes_when_command_matches_returns_process(
        #[case] executable: &str,
        #[case] path: Option<&str>,
        #[case] expected_id: TrackedProcessId,
        #[case] expected_label: &str,
    ) -> rootcause::Result<()> {
        let config = MuxrConfig::new()?;
        let process = config
            .tracked_process_for_cmd(executable, path)
            .ok_or_else(|| rootcause::report!("expected tracked process"))?;

        test_that::assert_that!(process.id, eq(expected_id));
        test_that::assert_that!(process.label, eq(expected_label));
        Ok(())
    }

    #[rstest::rstest]
    #[case::rg_codex("rg-codex", None)]
    #[case::notcodex("notcodex", None)]
    #[case::plain_node("node", None)]
    #[case::node_without_cursor_runtime("node", Some("/usr/local/bin/node"))]
    fn test_tracked_processes_when_command_does_not_match_returns_none(
        #[case] executable: &str,
        #[case] path: Option<&str>,
    ) {
        let config = MuxrConfig::new().unwrap();

        test_that::assert_that!(config.tracked_process_for_cmd(executable, path).is_none(), eq(true));
    }
}
