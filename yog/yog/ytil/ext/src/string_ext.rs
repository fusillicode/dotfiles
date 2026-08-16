//! Extensions for String and &str

pub trait StringExt {
    fn trim_end_at_with(&self, at: usize, with: Option<&str>) -> String;
}

impl<T: AsRef<str>> StringExt for T {
    fn trim_end_at_with(&self, at: usize, with: Option<&str>) -> String {
        let normalized = self.as_ref().split_whitespace().collect::<Vec<_>>().join(" ");
        let chars: Vec<char> = normalized.chars().collect();

        if chars.len() <= at {
            return normalized;
        }

        if at == 0 {
            return String::new();
        }

        if at == 1 {
            return "…".to_owned();
        }

        let mut trimmed: String = chars.into_iter().take(at.saturating_sub(1)).collect();
        trimmed.push_str(with.unwrap_or("…"));
        trimmed
    }
}

#[cfg(test)]
mod tests {
    use test_that::prelude::*;

    use super::*;

    #[rstest::rstest]
    #[case("hello world", 20, None, "hello world")]
    #[case("abcdefghijklmnopqrstuvwxyz", 5, None, "abcd…")]
    #[case("abc", 1, None, "…")]
    #[case("abc", 0, None, "")]
    #[case("abcdefghijklmnopqrstuvwxyz", 5, Some("!"), "abcd!")]
    fn test_trim_end_at_with_trims_as_expected(
        #[case] value: &str,
        #[case] max_chars: usize,
        #[case] with: Option<&str>,
        #[case] expected: &str,
    ) {
        assert_that!(value.trim_end_at_with(max_chars, with), eq(expected));
    }
}
