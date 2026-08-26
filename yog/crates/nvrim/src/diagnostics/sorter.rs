//! Diagnostic sorting utilities.
//!
//! Supplies a severity sorter used by statusline / floating windows. Missing severities default to
//! [`nvim_oxi::Integer::MIN`] to push them last relative to valid levels.

use nvim_oxi::Dictionary;
use nvim_oxi::Integer;

/// Sorts diagnostics by severity.
pub fn sort(mut lsp_diags: Vec<Dictionary>) -> Vec<Dictionary> {
    lsp_diags.sort_by_key(get_severity_or_default);
    lsp_diags
}

/// Gets the severity from a [`Dictionary`], defaulting to [`Integer::MIN`] if not present.
fn get_severity_or_default(dict: &Dictionary) -> Integer {
    dict.get("severity")
        .map_or(Ok(Integer::MIN), |o| Integer::try_from(o.clone()))
        .unwrap_or(Integer::MIN)
}
