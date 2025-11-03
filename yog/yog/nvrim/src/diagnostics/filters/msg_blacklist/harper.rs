//! Message blacklist configuration for the `Harper` LSP source.
//!
//! Suppresses noisy channel tokens and minor phrasing suggestions using a single [`MsgBlacklistFilter`].

use std::collections::HashMap;

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
    let blacklist: HashMap<_, _> = [
        "stderr",
        "stdout",
        "stdin",
        "insert `to` to complete the infinitive",
        "did you mean to spell `s` this way",
        "not the possessive `its`",
        "`argument` instead of `arg`",
    ]
    .into_iter()
    .map(|term| (term, None))
    .collect();

    vec![Box::new(MsgBlacklistFilter {
        source: "Harper",
        buf_path: None,
        blacklist,
    })]
}
