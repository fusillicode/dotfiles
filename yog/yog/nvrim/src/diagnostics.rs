//! Diagnostic processing utilities for LSP diagnostics.
//!
//! This module provides functionality to filter, format, and sort LSP diagnostics
//! received from language servers in Nvim.

use nvim_oxi::Dictionary;
use serde_repr::Deserialize_repr;
use strum::EnumIter;

use crate::dict;
use crate::fn_from;
use crate::oxi_ext::dict::DictionaryExt;

mod filter;
mod filters;
mod formatter;
mod sorter;

/// Diagnostic severity levels.
#[derive(Debug, Deserialize_repr, Hash, PartialEq, Eq, Copy, Clone, strum::Display, EnumIter)]
#[repr(u8)]
pub enum DiagnosticSeverity {
    /// Error severity.
    #[strum(to_string = "E")]
    Error = 1,
    /// Warning severity.
    #[strum(to_string = "W")]
    Warn = 2,
    /// Info severity.
    #[strum(to_string = "I")]
    Info = 3,
    /// Hint severity.
    #[strum(to_string = "H")]
    Hint = 4,
}

/// [`Dictionary`] of diagnostic processing helpers.
///
/// Includes:
/// - `format`: format function used by floating diagnostics window.
/// - `sort`: severity sorter (descending severity).
/// - `filter`: buffer / rules based filter.
/// - `config`: nested dictionary mirroring `vim.diagnostic.config({...})` currently defined in Lua.
pub fn dict() -> Dictionary {
    dict! {
        "format": fn_from!(formatter::format),
        "sort": fn_from!(sorter::sort),
        "filter": fn_from!(filter::filter),
        "config": dict! {
            "severity_sort": true,
            "signs": true,
            "underline": true,
            "update_in_insert": false,
            "virtual_text": false,
            "float": dict! {
                "anchor_bias": "above",
                "border": crate::style_opts::dict()
                    .get_dict(&["window"])
                    .unwrap_or_default()
                    .unwrap_or_default()
                    .get_t::<nvim_oxi::String>("border").unwrap_or_else(|_| "none".to_string()),
                "focusable": true,
                "format": fn_from!(formatter::format),
                "header": "",
                "prefix": "",
                "source": false,
                "suffix": "",
            }
        }
    }
}
