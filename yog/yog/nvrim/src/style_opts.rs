//! UI style options exposed as dictionaries for Lua consumption.
//!
//! Provides helper functions returning [`nvim_oxi::Dictionary`] values with
//! editor UI style configuration (window borders, previewer behaviour, etc.).

use nvim_oxi::Dictionary;

use crate::dict;
use crate::fn_from;

/// UI style options exported as an Nvim [`Dictionary`].
pub fn dict() -> Dictionary {
    dict! {
        "window": dict! {
            "border": "rounded",
        },
        "fzf_lua_previewer": fn_from!(fzf_lua_previewer)
    }
}

/// Returns the desired default `fzf-lua` previewer configuration as a [`Dictionary`].
///
/// # Arguments
/// - `previewer_kind` Optional override for the `previewer` key (falls back to the string literal `"builtin"` when.
///   [`Option::None`]).
fn fzf_lua_previewer(previewer_kind: Option<String>) -> Dictionary {
    dict! {
        "previewer": previewer_kind.unwrap_or_else(|| "builtin".to_string()),
        "winopts": dict! {
            "title": "",
            "height": 0.95,
            "preview": dict! {
                "default":  "builtin",
                "layout":  "vertical",
                "vertical":  "down:60%",
            }
        }
    }
}
