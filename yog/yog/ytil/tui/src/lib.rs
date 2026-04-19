//! Provide minimal TUI selection & prompt helpers built on [`skim`].
//!
//! Offer uniform, cancellable single / multi select prompts with fuzzy filtering and helpers
//! to derive a value from CLI args or fallback to an interactive selector.

#[cfg(not(target_arch = "wasm32"))]
pub use interactive::*;

#[cfg(not(target_arch = "wasm32"))]
pub mod git_branch;
#[cfg(not(target_arch = "wasm32"))]
mod interactive;

pub fn display_fixed_width(value: &str, max_chars: usize) -> String {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let chars: Vec<char> = normalized.chars().collect();

    if chars.len() <= max_chars {
        return normalized;
    }

    if max_chars == 0 {
        return String::new();
    }

    if max_chars == 1 {
        return "…".to_owned();
    }

    let mut trimmed: String = chars.into_iter().take(max_chars.saturating_sub(1)).collect();
    trimmed.push('…');
    trimmed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[rstest::rstest]
    #[case("hello world", 20, "hello world")]
    #[case("abcdefghijklmnopqrstuvwxyz", 5, "abcd…")]
    #[case("abc", 1, "…")]
    #[case("abc", 0, "")]
    fn display_fixed_width_trims_as_expected(#[case] value: &str, #[case] max_chars: usize, #[case] expected: &str) {
        pretty_assertions::assert_eq!(display_fixed_width(value, max_chars), expected);
    }
}
