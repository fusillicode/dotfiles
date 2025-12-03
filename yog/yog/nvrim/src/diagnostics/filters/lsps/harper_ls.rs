//! "Harper" LSP custom filter.
//!
//! Suppresses noisy diagnostics that cannot be filtered directly with "Harper".

use std::collections::HashMap;
use std::collections::HashSet;
use std::convert::identity;

use lit2::map;
use lit2::set;
use nvim_oxi::Dictionary;
use ytil_noxi::buffer::TextBoundary;

use crate::diagnostics::filters::BufferWithPath;
use crate::diagnostics::filters::DiagnosticLocation;
use crate::diagnostics::filters::DiagnosticsFilter;
use crate::diagnostics::filters::lsps::GetDiagMsgOutput;
use crate::diagnostics::filters::lsps::LspFilter;

pub struct HarperLsFilter<'a> {
    /// LSP diagnostic source name; only diagnostics from this source are eligible for blacklist matching.
    pub source: &'a str,
    /// Blacklist of messages per source.
    pub blacklist: HashMap<&'static str, HashSet<&'static str>>,
    /// Optional buffer path substring that must be contained within the buffer path for filtering to apply.
    pub path_substring: Option<&'a str>,
}

impl HarperLsFilter<'_> {
    /// Build Harper LSP diagnostic filters.
    ///
    /// Returns a vector of boxed [`DiagnosticsFilter`] configured for the Harper language server. Includes a single
    /// [`HarperLsFilter`] suppressing channel-related noise ("stderr", "stdout", "stdin").
    ///
    /// # Returns
    /// - [`Vec<Box<dyn DiagnosticsFilter>>`] Collection containing one configured [`HarperLsFilter`] for Harper.
    pub fn filters() -> Vec<Box<dyn DiagnosticsFilter>> {
        let blacklist = map! {
            "has ": set!["You may be missing a preposition here"],
            "stderr": set!["instead of"],
            "stdout": set!["instead of"],
            "stdin": set!["instead of"],
            "deduper": set!["Did you mean to spell"],
            "TODO": set!["Hyphenate"],
            "FIXME": set!["Did you mean `IME`"],
            "Resolve": set!["Insert `to` to complete the infinitive"],
            "foreground": set!["This sentence does not start with a capital letter"],
            "build": set!["This sentence does not start with a capital letter"],
            "args": set!["Use `argument` instead of `arg`"],
            "stack overflow": set!["Ensure proper capitalization of companies"],
            "over all": set!["closed compound `overall`"],
            "checkout": set!["not a compound noun"]
        };

        vec![Box::new(HarperLsFilter {
            source: "Harper",
            path_substring: None,
            blacklist,
        })]
    }
}

impl LspFilter for HarperLsFilter<'_> {
    fn path_substring(&self) -> Option<&str> {
        self.path_substring
    }

    fn source(&self) -> &str {
        self.source
    }
}

impl DiagnosticsFilter for HarperLsFilter<'_> {
    fn skip_diagnostic(&self, buf: &BufferWithPath, lsp_diag: &Dictionary) -> color_eyre::Result<bool> {
        let diag_msg = match self.get_diag_msg_or_skip(&buf.path, lsp_diag)? {
            GetDiagMsgOutput::Msg(diag_msg) => diag_msg,
            GetDiagMsgOutput::Skip => return Ok(false),
        };

        let diag_location = DiagnosticLocation::try_from(lsp_diag)?;

        let diag_text = buf
            .buffer
            .get_text_between(diag_location.start(), diag_location.end(), TextBoundary::Exact)?;

        Ok(self
            .blacklist
            .get(diag_text.as_str())
            .map(|blacklisted_msgs| {
                blacklisted_msgs
                    .iter()
                    .any(|blacklisted_msg| diag_msg.contains(blacklisted_msg))
            })
            .is_some_and(identity))
    }
}

#[cfg(test)]
mod tests {
    use ytil_noxi::buffer::mock::MockBuffer;

    use super::*;
    use crate::diagnostics::filters::BufferWithPath;

    #[test]
    fn skip_diagnostic_when_path_substring_pattern_not_matched_returns_false() {
        let filter = HarperLsFilter {
            source: "Harper",
            blacklist: map! {"stderr": set!["instead of"]},
            path_substring: Some("src/"),
        };
        let buf = create_buffer_with_path_and_content("tests/main.rs", vec!["stderr"]);
        let diag = dict! {
            source: "Harper",
            message: "instead of something",
            lnum: 0,
            col: 0,
            end_lnum: 0,
            end_col: 6,
        };
        assert2::let_assert!(Ok(res) = filter.skip_diagnostic(&buf, &diag));
        assert!(!res);
    }

    #[test]
    fn skip_diagnostic_when_source_mismatch_returns_false() {
        let filter = HarperLsFilter {
            source: "Harper",
            blacklist: map! {"stderr": set!["instead of"]},
            path_substring: None,
        };
        let buf = create_buffer_with_path_and_content("src/lib.rs", vec!["stderr"]);
        let diag = dict! {
            source: "Other",
            message: "instead of something",
            lnum: 0,
            col: 0,
            end_lnum: 0,
            end_col: 6,
        };
        assert2::let_assert!(Ok(res) = filter.skip_diagnostic(&buf, &diag));
        assert!(!res);
    }

    #[test]
    fn skip_diagnostic_when_diagnosed_text_not_in_blacklist_returns_false() {
        let filter = HarperLsFilter {
            source: "Harper",
            blacklist: map! {"stdout": set!["instead of"]},
            path_substring: None,
        };
        let buf = create_buffer_with_path_and_content("src/lib.rs", vec!["stderr"]);
        let diag = dict! {
            source: "Harper",
            message: "some message",
            lnum: 0,
            col: 0,
            end_lnum: 0,
            end_col: 6,
        };
        assert2::let_assert!(Ok(res) = filter.skip_diagnostic(&buf, &diag));
        assert!(!res);
    }

    #[test]
    fn skip_diagnostic_when_diagnosed_text_in_blacklist_but_message_no_match_returns_false() {
        let filter = HarperLsFilter {
            source: "Harper",
            blacklist: map! {"stderr": set!["instead of"]},
            path_substring: None,
        };
        let buf = create_buffer_with_path_and_content("src/lib.rs", vec!["stderr"]);
        let diag = dict! {
            source: "Harper",
            message: "some other message",
            lnum: 0,
            col: 0,
            end_lnum: 0,
            end_col: 6,
        };
        assert2::let_assert!(Ok(res) = filter.skip_diagnostic(&buf, &diag));
        assert!(!res);
    }

    #[test]
    fn skip_diagnostic_when_all_conditions_met_returns_true() {
        let filter = HarperLsFilter {
            source: "Harper",
            blacklist: map! {"stderr": set!["instead of"]},
            path_substring: None,
        };
        let buf = create_buffer_with_path_and_content("src/lib.rs", vec!["stderr"]);
        let diag = dict! {
            source: "Harper",
            message: "instead of something",
            lnum: 0,
            col: 0,
            end_lnum: 0,
            end_col: 6,
        };
        assert2::let_assert!(Ok(res) = filter.skip_diagnostic(&buf, &diag));
        assert!(res);
    }

    #[test]
    fn skip_diagnostic_when_diagnosed_text_cannot_be_extracted_returns_error() {
        let filter = HarperLsFilter {
            source: "Harper",
            blacklist: map! {"stderr": set!["instead of"]},
            path_substring: None,
        };
        let buf = create_buffer_with_path_and_content("src/lib.rs", vec!["short"]);
        let diag = dict! {
            source: "Harper",
            message: "instead of something",
            lnum: 1,
            col: 1,
            end_col: 7,
        };
        assert2::let_assert!(Err(err) = filter.skip_diagnostic(&buf, &diag));
        assert!(err.to_string().contains("missing dict value"));
        assert!(err.to_string().contains(r#""end_lnum""#));
    }

    #[test]
    fn skip_diagnostic_when_lnum_greater_than_end_lnum_returns_error() {
        let filter = HarperLsFilter {
            source: "Harper",
            blacklist: map! {"stderr": set!["instead of"]},
            path_substring: None,
        };
        let buf = create_buffer_with_path_and_content("src/lib.rs", vec!["hello world"]);
        let diag = dict! {
            source: "Harper",
            message: "some message",
            lnum: 1,
            col: 0,
            end_lnum: 0,
            end_col: 5,
        };
        assert2::let_assert!(Err(err) = filter.skip_diagnostic(&buf, &diag));
        assert!(err.to_string().contains("inconsistent line boundaries"));
        assert!(err.to_string().contains("lnum 1 > end_lnum 0"));
    }

    #[test]
    fn skip_diagnostic_when_col_greater_than_end_col_returns_error() {
        let filter = HarperLsFilter {
            source: "Harper",
            blacklist: map! {"stderr": set!["instead of"]},
            path_substring: None,
        };
        let buf = create_buffer_with_path_and_content("src/lib.rs", vec!["hello world"]);
        let diag = dict! {
            source: "Harper",
            message: "some message",
            lnum: 0,
            col: 5,
            end_lnum: 0,
            end_col: 0,
        };
        assert2::let_assert!(Err(err) = filter.skip_diagnostic(&buf, &diag));
        assert!(err.to_string().contains("inconsistent col boundaries"));
        assert!(err.to_string().contains("col 5 > end_col 0"));
    }

    #[test]
    fn skip_diagnostic_when_start_col_out_of_bounds_returns_error() {
        let filter = HarperLsFilter {
            source: "Harper",
            blacklist: map! {"stderr": set!["instead of"]},
            path_substring: None,
        };
        let buf = create_buffer_with_path_and_content("src/lib.rs", vec!["hi"]);
        let diag = dict! {
            source: "Harper",
            message: "some message",
            lnum: 0,
            col: 10,
            end_lnum: 0,
            end_col: 15,
        };
        assert2::let_assert!(Err(err) = filter.skip_diagnostic(&buf, &diag));
        assert!(err.to_string().contains("cannot extract substring"));
    }

    #[test]
    fn skip_diagnostic_when_empty_lines_returns_false() {
        let filter = HarperLsFilter {
            source: "Harper",
            blacklist: map! {"stderr": set!["instead of"]},
            path_substring: None,
        };
        let buf = create_buffer_with_path_and_content("src/lib.rs", vec![]);
        let diag = dict! {
            source: "Harper",
            message: "some message",
            lnum: 0,
            col: 0,
            end_lnum: 0,
            end_col: 5,
        };
        assert2::let_assert!(Ok(res) = filter.skip_diagnostic(&buf, &diag));
        assert!(!res);
    }

    fn create_buffer_with_path_and_content(path: &str, content: Vec<&str>) -> BufferWithPath {
        BufferWithPath {
            buffer: Box::new(MockBuffer::new(content.into_iter().map(str::to_string).collect())),
            path: path.to_string(),
        }
    }
}
