//! Diagnostic message substring blacklist filters.
//!
//! Implements per‑source case‑insensitive substring matching to hide low‑value diagnostics
//! (channel noise, trivial spelling hints). Supports optional buffer path gating for targeted
//! suppression.

use std::collections::HashMap;
use std::collections::HashSet;
use std::convert::identity;

use nvim_oxi::Dictionary;
use ytil_nvim_oxi::dict::DictionaryExt;

use crate::diagnostics::filters::BufferWithPath;
use crate::diagnostics::filters::DiagnosticsFilter;

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
    pub blacklist: HashMap<&'static str, Option<HashSet<&'static str>>>,
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
    fn skip_diagnostic(&self, buf: Option<&BufferWithPath>, lsp_diag: Option<&Dictionary>) -> color_eyre::Result<bool> {
        let (Some(buf), Some(lsp_diag)) = (buf, lsp_diag) else {
            return Ok(false);
        };
        if let Some(ref bp) = self.buf_path
            && !buf.path.contains(bp)
        {
            return Ok(false);
        }
        if let Some(source) = lsp_diag.get_opt_t::<nvim_oxi::String>("source")?
            && self.source != source
        {
            return Ok(false);
        }
        let msg = lsp_diag.get_t::<nvim_oxi::String>("message")?;
        let maybe_diagnosed_text = buf.get_diagnosed_text(lsp_diag)?;

        if maybe_diagnosed_text
            .and_then(|diagnosed_text| {
                self.blacklist
                    .get(diagnosed_text.as_str())
                    .and_then(|maybe_blacklisted_msgs| {
                        maybe_blacklisted_msgs
                            .as_ref()
                            .map(|set| set.iter().any(|s| s.contains(&msg)))
                    })
            })
            .is_some_and(identity)
        {
            return Ok(true);
        }
        Ok(self.blacklist.values().into_iter().any(|blacklisted_msgs| {
            blacklisted_msgs
                .as_ref()
                .map(|set| set.iter().any(|s| msg.contains(s)))
                .is_some_and(identity)
        }))
    }
}

#[cfg(test)]
mod tests {
    use ytil_nvim_oxi::buffer::mock::MockBuffer;

    use super::*;
    use crate::diagnostics::filters::BufferWithPath;

    #[test]
    fn skip_diagnostic_when_no_diagnostic_returns_false() {
        let filter = MsgBlacklistFilter {
            source: "foo",
            blacklist: HashMap::from([("foo", Some(HashSet::from(["stderr"])))]),
            buf_path: None,
        };
        assert2::let_assert!(Ok(res) = filter.skip_diagnostic(None, None));
        assert!(!res);
    }

    #[test]
    fn skip_diagnostic_when_buf_path_pattern_not_matched_returns_false() {
        let filter = MsgBlacklistFilter {
            source: "foo",
            blacklist: HashMap::from([("foo", Some(HashSet::from(["stderr"])))]),
            buf_path: Some("src/"),
        };
        let buf = create_buffer_with_path("tests/main.rs");
        let diag = dict! {
            source: "foo",
            message: "stderr something",
            lnum: 1,
            col: 1,
            end_lnum: 1,
            end_col: 1,
        };
        assert2::let_assert!(Ok(res) = filter.skip_diagnostic(Some(&buf), Some(&diag)));
        assert!(!res);
    }

    #[test]
    fn skip_diagnostic_when_source_mismatch_even_if_message_matches_returns_false() {
        let filter = MsgBlacklistFilter {
            source: "foo",
            blacklist: HashMap::from([("foo", Some(HashSet::from(["stderr"])))]),
            buf_path: None,
        };
        let buf = create_buffer_with_path("src/lib.rs");
        let diag = dict! {
            source: "other",
            message: "stderr noise",
            lnum: 1,
            col: 1,
            end_lnum: 1,
            end_col: 1,
        };
        assert2::let_assert!(Ok(res) = filter.skip_diagnostic(Some(&buf), Some(&diag)));
        assert!(!res);
    }

    #[test]
    fn skip_diagnostic_when_blacklisted_substring_with_all_preconditions_met_returns_true() {
        let filter = MsgBlacklistFilter {
            source: "foo",
            blacklist: HashMap::from([("foo", Some(HashSet::from(["stderr", "stdout"])))]),
            buf_path: Some("src"),
        };
        let buf = create_buffer_with_path("/project/src/lib.rs");
        let diag = dict! {
            source: "foo",
            message: "stdout channel mention",
            lnum: 1,
            col: 1,
            end_lnum: 1,
            end_col: 1
        };
        assert2::let_assert!(Ok(res) = filter.skip_diagnostic(Some(&buf), Some(&diag)));
        assert!(res);
    }

    #[test]
    fn skip_diagnostic_when_message_contains_blacklisted_substring_returns_true() {
        let filter = MsgBlacklistFilter {
            source: "foo",
            blacklist: HashMap::from([("foo", Some(HashSet::from(["STDERR"])))]),
            buf_path: None,
        };
        let buf = create_buffer_with_path("file.rs");
        let diag = dict! {
            source: "foo",
            message: "STDERR reported",
            lnum: 1,
            col: 1,
            end_lnum: 1,
            end_col: 1,
        };
        assert2::let_assert!(Ok(res) = filter.skip_diagnostic(Some(&buf), Some(&diag)));
        assert!(res);
    }

    #[test]
    fn skip_diagnostic_when_message_not_in_blacklist_returns_false() {
        let filter = MsgBlacklistFilter {
            source: "foo",
            blacklist: HashMap::from([("foo", Some(HashSet::from(["stderr"])))]),
            buf_path: None,
        };
        let buf = create_buffer_with_path("file.rs");
        let diag = dict! {
            source: "foo",
            message: "regular info",
            lnum: 1,
            col: 1,
            end_lnum: 1,
            end_col: 1,
        };
        assert2::let_assert!(Ok(res) = filter.skip_diagnostic(Some(&buf), Some(&diag)));
        assert!(!res);
    }

    #[test]
    fn skip_diagnostic_when_missing_source_key_and_message_blacklisted_returns_true() {
        let filter = MsgBlacklistFilter {
            source: "foo",
            blacklist: HashMap::from([("foo", Some(HashSet::from(["stderr"])))]),
            buf_path: None,
        };
        // Only message key present. Missing source should not produce an error; blacklist still applies.
        let buf = create_buffer_with_path("file.rs");
        let diag = dict! {
            message: "stderr reported",
            lnum: 1,
            col: 1,
            end_lnum: 1,
            end_col: 1,
        };
        assert2::let_assert!(Ok(res) = filter.skip_diagnostic(Some(&buf), Some(&diag)));
        assert!(res);
    }

    #[test]
    fn skip_diagnostic_when_missing_source_and_message_not_blacklisted_returns_false() {
        let filter = MsgBlacklistFilter {
            source: "foo",
            blacklist: HashMap::from([("foo", Some(HashSet::from(["stderr"])))]),
            buf_path: None,
        };
        let buf = create_buffer_with_path("file.rs");
        let diag = dict! {
            message: "ordinary info",
            lnum: 1,
            col: 1,
            end_lnum: 1,
            end_col: 1,
        };
        assert2::let_assert!(Ok(res) = filter.skip_diagnostic(Some(&buf), Some(&diag)));
        assert!(!res);
    }

    #[test]
    fn skip_diagnostic_when_overlapping_blacklist_substrings_returns_true() {
        let filter = MsgBlacklistFilter {
            source: "foo",
            blacklist: HashMap::from([("foo", Some(HashSet::from(["ERROR", "err"])))]),
            buf_path: None,
        };
        let buf = create_buffer_with_path("file.rs");
        let diag = dict! {
            message: "An ERROR occurred",
            lnum: 1,
            col: 1,
            end_lnum: 1,
            end_col: 1,
        };
        assert2::let_assert!(Ok(res) = filter.skip_diagnostic(Some(&buf), Some(&diag)));
        assert!(res);
    }

    #[test]
    fn skip_diagnostic_when_missing_message_key_returns_error() {
        let filter = MsgBlacklistFilter {
            source: "foo",
            blacklist: HashMap::from([("foo", Some(HashSet::from(["stderr"])))]),
            buf_path: None,
        };
        // Only source key present.
        let buf = create_buffer_with_path("file.rs");
        let diag = dict! {
            source: "foo",
            lnum: 1,
            col: 1,
            end_lnum: 1,
            end_col: 1,
        };
        assert2::let_assert!(Err(err) = filter.skip_diagnostic(Some(&buf), Some(&diag)));
        let msg = err.to_string();
        assert!(msg.starts_with("missing dict value |"), "actual: {msg}");
        assert!(msg.contains("query=[\n    \"message\",\n]"), "actual: {msg}");
    }

    #[test]
    fn skip_diagnostic_when_source_wrong_type_returns_error() {
        let filter = MsgBlacklistFilter {
            source: "foo",
            blacklist: HashMap::from([("foo", Some(HashSet::from(["stderr"])))]),
            buf_path: None,
        };
        // Wrong type for source (integer) plus valid message.
        let buf = create_buffer_with_path("file.rs");
        let diag = dict! {
            source: 42,
            message: "stderr noise",
            lnum: 1,
            col: 1,
            end_lnum: 1,
            end_col: 1,
        };
        assert2::let_assert!(Err(err) = filter.skip_diagnostic(Some(&buf), Some(&diag)));
        let msg = err.to_string();
        assert!(msg.contains("is Integer but String was expected"), "actual: {msg}");
        assert!(msg.contains("key \"source\""), "actual: {msg}");
    }

    #[test]
    fn skip_diagnostic_when_message_wrong_type_returns_error() {
        let filter = MsgBlacklistFilter {
            source: "foo",
            blacklist: HashMap::from([("foo", Some(HashSet::from(["stderr"])))]),
            buf_path: None,
        };
        // Wrong type for message (integer) plus valid source.
        let buf = create_buffer_with_path("file.rs");
        let diag = dict! {
            source: "foo",
            message: 7,
            lnum: 1,
            col: 1,
            end_lnum: 1,
            end_col: 1,
        };
        assert2::let_assert!(Err(err) = filter.skip_diagnostic(Some(&buf), Some(&diag)));
        let msg = err.to_string();
        assert!(msg.contains("is Integer but String was expected"), "actual: {msg}");
        assert!(msg.contains("key \"message\""), "actual: {msg}");
    }

    #[test]
    fn skip_diagnostic_when_diagnosed_text_matches_has_space_and_message_matches_preposition_suggestion_returns_true() {
        let filter = MsgBlacklistFilter {
            source: "test_source",
            blacklist: [(
                "has ",
                Some(
                    vec!["You may be missing a preposition here"]
                        .into_iter()
                        .collect::<HashSet<_>>(),
                ),
            )]
            .into_iter()
            .collect(),
            buf_path: None,
        };
        let buf = BufferWithPath {
            buffer: Box::new(MockBuffer(vec!["has something".to_string()])),
            path: "test.rs".to_string(),
        };
        let diag = dict! {
            source: "test_source",
            message: "You may be missing a preposition here",
            lnum: 0,
            col: 0,
            end_lnum: 0,
            end_col: 4,
        };
        assert2::let_assert!(Ok(res) = filter.skip_diagnostic(Some(&buf), Some(&diag)));
        assert!(res);
    }

    fn create_buffer_with_path(path: &str) -> BufferWithPath {
        BufferWithPath {
            buffer: Box::new(MockBuffer(vec![])),
            path: path.to_string(),
        }
    }
}
