use std::sync::LazyLock;

use regex::Error;
use regex::Regex;

const ELAPSED: &str = r"(?:[0-9]+h(?:\s+[0-9]+m)?(?:\s+[0-9]+s)?|[0-9]+m(?:\s+[0-9]+s)?|[0-9]+s)";

static BUSY: LazyLock<Result<Regex, Error>> =
    LazyLock::new(|| Regex::new(&format!(r"\AWorking \(\s*{ELAPSED}\s*•\s*\S+ to interrupt\s*\)\z")));

static ATTENTION: LazyLock<Result<Regex, Error>> = LazyLock::new(|| {
    Regex::new(&format!(
        r"\AWorked for \s*{ELAPSED}\s*•\s*(?:[01][0-9]|2[0-3]):[0-5][0-9]\z"
    ))
});

pub(super) fn busy(line: &str) -> bool {
    BUSY.as_ref().is_ok_and(|pattern| pattern.is_match(status_text(line)))
}

pub(super) fn needs_attention(line: &str) -> Option<&str> {
    let status = status_text(line);
    ATTENTION
        .as_ref()
        .is_ok_and(|pattern| pattern.is_match(status))
        .then_some(status)
}

fn status_text(line: &str) -> &str {
    line.trim_matches(|character: char| {
        character.is_whitespace()
            || matches!(
                character,
                '─' | '━' | '•' | '·' | '⠋' | '⠙' | '⠹' | '⠸' | '⠼' | '⠴' | '⠦' | '⠧' | '⠇' | '⠏'
            )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[rstest::rstest]
    #[case("Working (6m 35s • ctrl+x to interrupt)")]
    #[case("• Working (0s • esc to interrupt)")]
    #[case("⠹ Working (1h 2m 3s • ctrl+c to interrupt) ")]
    #[case("Working (1h • esc to interrupt)")]
    #[case("Working (2m • ctrl+c to interrupt)")]
    #[case("Working ( 1h\t2m 3s \t•\tctrl+x to interrupt )")]
    fn test_busy_when_status_is_valid_returns_busy(#[case] text: &str) {
        assert!(busy(text));
    }

    #[rstest::rstest]
    #[case("Worked for 22m 38s • 11:37", "Worked for 22m 38s • 11:37")]
    #[case("── Worked for 0s • 00:01 ───", "Worked for 0s • 00:01")]
    #[case("Worked for 1h 2m 3s • 23:59", "Worked for 1h 2m 3s • 23:59")]
    #[case("\t── Worked for   22m\t38s  •\t11:37 ── ", "Worked for   22m\t38s  •\t11:37")]
    fn test_needs_attention_when_completion_is_valid_returns_status_text(#[case] text: &str, #[case] expected: &str) {
        assert_eq!(needs_attention(text), Some(expected));
    }

    #[rstest::rstest]
    #[case("Working (6m 35s • ctrl+x to interrupt)")]
    #[case("› Worked for 22m 38s • 11:37")]
    #[case("example: Worked for 22m 38s • 11:37")]
    #[case("Worked for 1s • 24:00")]
    #[case("Worked for 1s • 12:60")]
    #[case("Worked for 1s • 12:30 quoted text")]
    #[case("Worked for • 12:30")]
    fn test_needs_attention_when_text_is_not_a_completion_returns_none(#[case] text: &str) {
        assert_eq!(needs_attention(text), None);
    }

    #[rstest::rstest]
    #[case("› Working (6m 35s • ctrl+x to interrupt)")]
    #[case("example: Working (6m 35s • ctrl+x to interrupt)")]
    #[case("Working (later • ctrl+x to interrupt)")]
    #[case("Working (1s 2m • ctrl+x to interrupt)")]
    #[case("Working (1s • to interrupt)")]
    #[case("Worked for 22m 38s • 11:37")]
    fn test_busy_when_text_is_not_a_working_status_returns_false(#[case] text: &str) {
        assert!(!busy(text));
    }
}
