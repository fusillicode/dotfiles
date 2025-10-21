//! Message blacklist configuration for the `Harper` LSP source.
//!
//! Suppresses noisy channel tokens and minor phrasing suggestions using a single [`MsgBlacklistFilter`].

use crate::diagnostics::filters::DiagnosticsFilter;
use crate::diagnostics::filters::msg_blacklist::MsgBlacklistFilter;

/// Build Harper LSP diagnostic filters.
///
/// Returns a vector of boxed [`DiagnosticsFilter`] configured for the Harper language server. Includes a single
/// [`MsgBlacklistFilter`] suppressing channel-related noise ("stderr", "stdout", "stdin").
///
/// # Returns
/// - `Vec<Box<dyn DiagnosticsFilter>>`: Collection containing one configured [`MsgBlacklistFilter`] for Harper.
pub fn filters() -> Vec<Box<dyn DiagnosticsFilter>> {
    vec![Box::new(MsgBlacklistFilter {
        source: "Harper",
        buf_path: None,
        blacklist: vec![
            "stderr".into(),
            "stdout".into(),
            "stdin".into(),
            "insert `to` to complete the infinitive".into(),
            "did you mean to spell `s` this way".into(),
            "not the possessive `its`".into(),
            "`argument` instead of `arg`".into(),
        ],
    })]
}
