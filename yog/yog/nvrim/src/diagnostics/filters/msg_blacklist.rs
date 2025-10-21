//! Diagnostic message substring blacklist filters.
//!
//! Implements per‑source case‑insensitive substring matching to hide low‑value diagnostics
//! (channel noise, trivial spelling hints). Supports optional buffer path gating for targeted
//! suppression.

use nvim_oxi::Dictionary;

use crate::diagnostics::filters::DiagnosticsFilter;
use crate::oxi_ext::dict::DictionaryExt;

pub mod harper;
pub mod typos;

/// Filters out diagnostics whose messages contain any blacklisted substrings.
///
/// Filters diagnostics whose lowercase message contains any of the configured blacklist
/// substrings, provided:
/// - A diagnostic is present.
/// - The optional buffer path pattern (if set) is contained in the buffer path.
/// - The diagnostic's `source` equals [`MsgBlacklistFilter::source`].
///
/// This struct is configured with:
/// - [`MsgBlacklistFilter::source`]: LSP source name used for source-difference gating.
/// - [`MsgBlacklistFilter::blacklist`]: Case-insensitive substrings (stored pre-lowercased; message is lowered once).
/// - [`MsgBlacklistFilter::buf_path`]: Optional substring pattern that must appear within the buffer path for filtering
///   to apply.
///
/// See [`DiagnosticsFilter`] for trait integration.
///
/// # Rationale
/// Some language servers or tools emit noisy diagnostics (e.g. repeated I/O channel mentions).
/// Keeping them out improves signal without mutating upstream server configuration.
///
/// # Future Work
/// - Source gating currently requires the diagnostic `source` to equal `MsgBlacklistFilter::source`; blacklist only
///   applies to that single source. Future improvement: support configurable mode (inclusive vs exclusive) or multiple
///   sources.
/// - Support regex-based patterns for messages and buffer paths.
/// - Provide statistics on filtered counts for observability.
pub struct MsgBlacklistFilter<'a> {
    /// LSP diagnostic source name; only diagnostics from this source are eligible for blacklist matching.
    pub source: &'a str,
    /// Blacklist of messages per source.
    pub blacklist: Vec<String>,
    /// Optional buffer path substring that must be contained within the buffer path for filtering to apply.
    pub buf_path: Option<&'a str>,
}

impl DiagnosticsFilter for MsgBlacklistFilter<'_> {
    /// Returns true if the diagnostic message is blacklisted.
    ///
    /// Behavior:
    /// - Missing diagnostic: `Ok(false)`.
    /// - `buf_path` constraint set but not found in `buf_path`: `Ok(false)`.
    /// - Missing `source` key: treated as wildcard (still eligible for blacklist).
    /// - Present `source` different from [`MsgBlacklistFilter::source`]: `Ok(false)`.
    /// - Lowercased `message` contains any blacklist entry: `Ok(true)`.
    /// - Otherwise: `Ok(false)`.
    ///
    /// # Errors
    /// - `message` key is missing.
    /// - `message` value has unexpected type (must be [`String`]).
    /// - `source` key present but has unexpected type (must be [`String`]).
    fn skip_diagnostic(&self, buf_path: &str, lsp_diag: Option<&Dictionary>) -> color_eyre::Result<bool> {
        let Some(lsp_diag) = lsp_diag else {
            return Ok(false);
        };
        if let Some(ref bp) = self.buf_path
            && !buf_path.contains(bp)
        {
            return Ok(false);
        }
        if let Some(source) = lsp_diag.get_opt_t::<nvim_oxi::String>("source")?
            && self.source != source
        {
            return Ok(false);
        }
        let msg = lsp_diag.get_t::<nvim_oxi::String>("message")?.to_lowercase();
        if self.blacklist.iter().any(|b| msg.contains(b)) {
            return Ok(true);
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dict;

    #[test]
    fn skip_returns_false_when_no_diagnostic() {
        let filter = MsgBlacklistFilter {
            source: "foo",
            blacklist: vec!["stderr".into()],
            buf_path: None,
        };
        assert2::let_assert!(Ok(res) = filter.skip_diagnostic("any/path", None));
        assert!(!res);
    }

    #[test]
    fn skip_returns_false_when_buf_path_pattern_not_matched() {
        let filter = MsgBlacklistFilter {
            source: "foo",
            blacklist: vec!["stderr".into()],
            buf_path: Some("src/"),
        };
        let diag = dict! { source: "foo", message: "stderr something" };
        assert2::let_assert!(Ok(res) = filter.skip_diagnostic("tests/main.rs", Some(&diag)));
        assert!(!res);
    }

    #[test]
    fn skip_returns_false_when_source_mismatch_even_if_message_matches() {
        let filter = MsgBlacklistFilter {
            source: "foo",
            blacklist: vec!["stderr".into()],
            buf_path: None,
        };
        let diag = dict! { source: "other", message: "stderr noise" };
        assert2::let_assert!(Ok(res) = filter.skip_diagnostic("src/lib.rs", Some(&diag)));
        assert!(!res);
    }

    #[test]
    fn skip_returns_true_on_blacklisted_substring_with_all_preconditions_met() {
        let filter = MsgBlacklistFilter {
            source: "foo",
            blacklist: vec!["stderr".into(), "stdout".into()],
            buf_path: Some("src"),
        };
        let diag = dict! { source: "foo", message: "stdout channel mention" };
        assert2::let_assert!(Ok(res) = filter.skip_diagnostic("/project/src/lib.rs", Some(&diag)));
        assert!(res);
    }

    #[test]
    fn skip_matches_case_insensitive_message() {
        let filter = MsgBlacklistFilter {
            source: "foo",
            blacklist: vec!["stderr".into()],
            buf_path: None,
        };
        let diag = dict! { source: "foo", message: "STDERR reported" };
        assert2::let_assert!(Ok(res) = filter.skip_diagnostic("file.rs", Some(&diag)));
        assert!(res);
    }

    #[test]
    fn skip_returns_false_when_message_not_in_blacklist() {
        let filter = MsgBlacklistFilter {
            source: "foo",
            blacklist: vec!["stderr".into()],
            buf_path: None,
        };
        let diag = dict! { source: "foo", message: "regular info" };
        assert2::let_assert!(Ok(res) = filter.skip_diagnostic("file.rs", Some(&diag)));
        assert!(!res);
    }

    #[test]
    fn skip_returns_true_when_missing_source_key_and_message_blacklisted() {
        let filter = MsgBlacklistFilter {
            source: "foo",
            blacklist: vec!["stderr".into()],
            buf_path: None,
        };
        // Only message key present. Missing source should not produce an error; blacklist still applies.
        let diag = dict! { message: "stderr reported" };
        assert2::let_assert!(Ok(res) = filter.skip_diagnostic("file.rs", Some(&diag)));
        assert!(res);
    }

    #[test]
    fn skip_returns_false_when_missing_source_and_message_not_blacklisted() {
        let filter = MsgBlacklistFilter {
            source: "foo",
            blacklist: vec!["stderr".into()],
            buf_path: None,
        };
        let diag = dict! { message: "ordinary info" };
        assert2::let_assert!(Ok(res) = filter.skip_diagnostic("file.rs", Some(&diag)));
        assert!(!res);
    }

    #[test]
    fn skip_returns_true_on_overlapping_blacklist_substrings() {
        let filter = MsgBlacklistFilter {
            source: "foo",
            blacklist: vec!["error".into(), "err".into()],
            buf_path: None,
        };
        let diag = dict! { message: "An ERROR occurred" };
        assert2::let_assert!(Ok(res) = filter.skip_diagnostic("file.rs", Some(&diag)));
        assert!(res);
    }

    #[test]
    fn skip_returns_error_when_missing_message_key() {
        let filter = MsgBlacklistFilter {
            source: "foo",
            blacklist: vec!["stderr".into()],
            buf_path: None,
        };
        // Only source key present.
        let diag = dict! { source: "foo" };
        assert2::let_assert!(Err(err) = filter.skip_diagnostic("file.rs", Some(&diag)));
        let msg = err.to_string();
        assert!(msg.starts_with("missing dict value |"), "actual: {msg}");
        assert!(msg.contains("query=[\n    \"message\",\n]"), "actual: {msg}");
    }

    #[test]
    fn skip_returns_error_when_source_wrong_type() {
        let filter = MsgBlacklistFilter {
            source: "foo",
            blacklist: vec!["stderr".into()],
            buf_path: None,
        };
        // Wrong type for source (integer) plus valid message.
        let diag = dict! { source: 42, message: "stderr noise" };
        assert2::let_assert!(Err(err) = filter.skip_diagnostic("file.rs", Some(&diag)));
        let msg = err.to_string();
        assert!(msg.contains("is Integer but String was expected"), "actual: {msg}");
        assert!(msg.contains("key \"source\""), "actual: {msg}");
    }

    #[test]
    fn skip_returns_error_when_message_wrong_type() {
        let filter = MsgBlacklistFilter {
            source: "foo",
            blacklist: vec!["stderr".into()],
            buf_path: None,
        };
        // Wrong type for message (integer) plus valid source.
        let diag = dict! { source: "foo", message: 7 };
        assert2::let_assert!(Err(err) = filter.skip_diagnostic("file.rs", Some(&diag)));
        let msg = err.to_string();
        assert!(msg.contains("is Integer but String was expected"), "actual: {msg}");
        assert!(msg.contains("key \"message\""), "actual: {msg}");
    }
}
