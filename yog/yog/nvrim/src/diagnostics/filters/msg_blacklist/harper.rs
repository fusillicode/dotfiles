//! Message blacklist configuration for the `Harper` LSP source.
//!
//! Suppresses noisy channel tokens and minor phrasing suggestions using a single [`MsgBlacklistFilter`].

use std::collections::HashMap;
use std::collections::HashSet;

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
        (
            "has ",
            Some(
                vec!["You may be missing a preposition here"]
                    .into_iter()
                    .collect::<HashSet<_>>(),
            ),
        ),
        ("stderr", Some(vec!["instead of"].into_iter().collect::<HashSet<_>>())),
        ("stdout", Some(vec!["instead of"].into_iter().collect::<HashSet<_>>())),
        ("stdin", Some(vec!["instead of"].into_iter().collect::<HashSet<_>>())),
    ]
    .into_iter()
    .collect();

    vec![Box::new(MsgBlacklistFilter {
        source: "Harper",
        buf_path: None,
        blacklist,
    })]
}
