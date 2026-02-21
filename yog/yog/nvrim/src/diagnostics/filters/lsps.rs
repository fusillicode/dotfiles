//! Filter diagnostics based on LSP source and buffer path.
//!
//! Provides the [`LspFilter`] trait for filtering diagnostics by LSP source and buffer path,
//! along with implementations for specific LSPs like Harper and Typos.

use nvim_oxi::Dictionary;
use ytil_noxi::dict::DictionaryExt as _;

pub mod harper_ls;
pub mod typos_lsp;

/// Output of diagnostic message extraction or skip decision.
#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
pub enum GetDiagMsgOutput {
    /// Diagnostic message extracted successfully.
    Msg(String),
    /// Skip this diagnostic.
    Skip,
}

/// Common interface for LSP-specific diagnostic filters.
///
/// Provides utilities for path and source matching before message extraction.
pub trait LspFilter {
    /// Optional buffer path substring required for filtering.
    ///
    /// If present, filtering only applies to buffers containing this substring.
    fn path_substring(&self) -> Option<&str>;

    /// LSP source name for this filter.
    fn source(&self) -> &str;

    /// Extract diagnostic message or decide to skip.
    ///
    /// Checks path substring and source match, then extracts message if applicable.
    ///
    /// # Errors
    /// - Missing or invalid "source" key.
    /// - Missing or invalid "message" key.
    fn get_diag_msg_or_skip(&self, buf_path: &str, lsp_diag: &Dictionary) -> rootcause::Result<GetDiagMsgOutput> {
        if self
            .path_substring()
            .is_some_and(|path_substring| !buf_path.contains(path_substring))
        {
            return Ok(GetDiagMsgOutput::Skip);
        }
        let maybe_diag_source = lsp_diag.get_opt_t::<nvim_oxi::String>("source")?;
        if maybe_diag_source.is_none_or(|diag_source| !diag_source.contains(self.source())) {
            return Ok(GetDiagMsgOutput::Skip);
        }
        Ok(GetDiagMsgOutput::Msg(lsp_diag.get_t::<nvim_oxi::String>("message")?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_diag_msg_or_skip_when_buf_path_not_matched_returns_skip() {
        let filter = TestFilter {
            source: "Test",
            path_substring: Some("src/"),
        };
        let diag = dict! {
            source: "Test",
            message: "some message",
        };
        assert2::assert!(let Ok(result) = filter.get_diag_msg_or_skip("tests/main.rs", &diag));
        pretty_assertions::assert_eq!(result, GetDiagMsgOutput::Skip);
    }

    #[test]
    fn get_diag_msg_or_skip_when_buf_path_matched_but_source_none_returns_skip() {
        let filter = TestFilter {
            source: "Test",
            path_substring: Some("src/"),
        };
        let diag = dict! {
            message: "some message",
        };
        assert2::assert!(let Ok(result) = filter.get_diag_msg_or_skip("src/main.rs", &diag));
        pretty_assertions::assert_eq!(result, GetDiagMsgOutput::Skip);
    }

    #[test]
    fn get_diag_msg_or_skip_when_buf_path_matched_but_source_mismatch_returns_skip() {
        let filter = TestFilter {
            source: "Test",
            path_substring: Some("src/"),
        };
        let diag = dict! {
            source: "Other",
            message: "some message",
        };
        assert2::assert!(let Ok(result) = filter.get_diag_msg_or_skip("src/main.rs", &diag));
        pretty_assertions::assert_eq!(result, GetDiagMsgOutput::Skip);
    }

    #[test]
    fn get_diag_msg_or_skip_when_buf_path_and_source_matches_returns_msg() {
        let filter = TestFilter {
            source: "Test",
            path_substring: Some("src/"),
        };
        let diag = dict! {
            source: "Test",
            message: "some message",
        };
        assert2::assert!(let Ok(result) = filter.get_diag_msg_or_skip("src/main.rs", &diag));
        pretty_assertions::assert_eq!(result, GetDiagMsgOutput::Msg("some message".to_string()));
    }

    #[test]
    fn get_diag_msg_or_skip_when_no_buf_path_and_source_matches_returns_msg() {
        let filter = TestFilter {
            source: "Test",
            path_substring: None,
        };
        let diag = dict! {
            source: "Test",
            message: "another message",
        };
        assert2::assert!(let Ok(result) = filter.get_diag_msg_or_skip("any/path.rs", &diag));
        pretty_assertions::assert_eq!(result, GetDiagMsgOutput::Msg("another message".to_string()));
    }

    #[test]
    fn get_diag_msg_or_skip_when_source_contains_filter_source_returns_msg() {
        let filter = TestFilter {
            source: "Test",
            path_substring: None,
        };
        let diag = dict! {
            source: "TestLSP",
            message: "some message",
        };
        assert2::assert!(let Ok(result) = filter.get_diag_msg_or_skip("any/path.rs", &diag));
        pretty_assertions::assert_eq!(result, GetDiagMsgOutput::Msg("some message".to_string()));
    }

    struct TestFilter {
        source: &'static str,
        path_substring: Option<&'static str>,
    }

    impl LspFilter for TestFilter {
        fn path_substring(&self) -> Option<&str> {
            self.path_substring
        }

        fn source(&self) -> &str {
            self.source
        }
    }
}
