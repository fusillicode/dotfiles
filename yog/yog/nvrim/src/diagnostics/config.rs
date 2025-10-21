//! Neovim diagnostics configuration dictionary builder.
//!
//! Produces a Lua‑consumable `config` dict mirroring `vim.diagnostic.config({...})` with custom float
//! window border, severity sorting, and sign text derived from [`DiagnosticSeverity`] variants.

use nvim_oxi::Array;
use nvim_oxi::Dictionary;
use nvim_oxi::Object;
use strum::IntoEnumIterator;

use crate::diagnostics::DiagnosticSeverity;
use crate::diagnostics::dict;
use crate::diagnostics::formatter;
use crate::fn_from;
use crate::oxi_ext::dict::DictionaryExt;

/// Nvim diagnostics configuration.
pub fn get() -> Dictionary {
    let texts_signs: Array = DiagnosticSeverity::iter()
        .map(|s| Object::from(s.to_string()))
        .collect();

    dict! {
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
        },
        "signs": dict! {
            "text": texts_signs
        }
    }
}
