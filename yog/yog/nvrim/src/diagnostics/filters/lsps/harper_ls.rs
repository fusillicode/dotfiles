//! Message blacklist configuration for the `Harper` LSP source.
//!
//! Suppresses noisy channel tokens and minor phrasing suggestions using a single [`MsgBlacklistFilter`].

use std::collections::HashMap;
use std::collections::HashSet;
use std::convert::identity;

use color_eyre::eyre::eyre;
use nvim_oxi::Dictionary;

use super::GetDiagMsgOutput;
use crate::diagnostics::filters::BufferWithPath;
use crate::diagnostics::filters::DiagnosticsFilter;
use crate::diagnostics::filters::lsps::LspFilter;

pub struct HarperLsFilter<'a> {
    /// LSP diagnostic source name; only diagnostics from this source are eligible for blacklist matching.
    pub source: &'a str,
    /// Blacklist of messages per source.
    pub blacklist: HashMap<&'static str, HashSet<&'static str>>,
    /// Optional buffer path substring that must be contained within the buffer path for filtering to apply.
    pub buf_path: Option<&'a str>,
}

impl HarperLsFilter<'_> {
    /// Build Harper LSP diagnostic filters.
    ///
    /// Returns a vector of boxed [`DiagnosticsFilter`] configured for the Harper language server. Includes a single
    /// [`MsgBlacklistFilter`] suppressing channel-related noise ("stderr", "stdout", "stdin").
    ///
    /// # Returns
    /// - `Vec<Box<dyn DiagnosticsFilter>>`: Collection containing one configured [`MsgBlacklistFilter`] for Harper.
    pub fn filters() -> Vec<Box<dyn DiagnosticsFilter>> {
        let blacklist: HashMap<_, _> = [
            (
                "has ",
                vec!["You may be missing a preposition here"].into_iter().collect(),
            ),
            ("stderr", vec!["instead of"].into_iter().collect()),
            ("stdout", vec!["instead of"].into_iter().collect()),
            ("stdin", vec!["instead of"].into_iter().collect()),
            ("deduper", vec!["Did you mean to spell"].into_iter().collect()),
            ("TODO", vec!["Hyphenate"].into_iter().collect()),
            ("FIXME", vec!["Did you mean `IME`"].into_iter().collect()),
        ]
        .into_iter()
        .collect();

        vec![Box::new(HarperLsFilter {
            source: "Harper",
            buf_path: None,
            blacklist,
        })]
    }
}

impl LspFilter for HarperLsFilter<'_> {
    fn buf_path(&self) -> Option<&str> {
        self.buf_path
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

        let diagnosed_text = buf.get_diagnosed_text(lsp_diag)?.ok_or_else(|| {
            eyre!(
                "missing diagnosed text for {} filter | lsp_diag={lsp_diag:#?}",
                self.source
            )
        })?;

        Ok(self
            .blacklist
            .get(diagnosed_text.as_str())
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
    use std::collections::HashSet;

    use ytil_nvim_oxi::buffer::mock::MockBuffer;

    use super::*;
    use crate::diagnostics::filters::BufferWithPath;

    #[test]
    fn skip_diagnostic_when_buf_path_pattern_not_matched_returns_false() {
        let filter = HarperLsFilter {
            source: "Harper",
            blacklist: [("stderr", HashSet::from(["instead of"]))].into(),
            buf_path: Some("src/"),
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
            blacklist: [("stderr", HashSet::from(["instead of"]))].into(),
            buf_path: None,
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
            blacklist: [("stdout", HashSet::from(["instead of"]))].into(),
            buf_path: None,
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
            blacklist: [("stderr", HashSet::from(["instead of"]))].into(),
            buf_path: None,
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
            blacklist: [("stderr", HashSet::from(["instead of"]))].into(),
            buf_path: None,
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
            blacklist: [("stderr", HashSet::from(["instead of"]))].into(),
            buf_path: None,
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
        assert!(err.to_string().contains("missing diagnosed text"));
    }

    fn create_buffer_with_path_and_content(path: &str, content: Vec<&str>) -> BufferWithPath {
        BufferWithPath {
            buffer: Box::new(MockBuffer(content.into_iter().map(str::to_string).collect())),
            path: path.to_string(),
        }
    }
}
