//! UI style options exposed as dictionaries for Lua consumption.
//!
//! Provides helper functions returning [`nvim_oxi::Dictionary`] values with
//! editor UI style configuration (window borders, previewer behavior, etc.).

use nvim_oxi::Dictionary;

/// UI style options exported as an Nvim [`Dictionary`].
pub fn dict() -> Dictionary {
    dict! {
        "window": dict! {
            "border": "rounded",
        },
        "fzf_lua": fn_from!(fzf_lua),
    }
}

/// Returns the desired default `fzf-lua` previewer configuration as a [`Dictionary`].
///
/// # Arguments
/// - `previewer_kind` Optional override for the `previewer` key (falls back to the string literal `"builtin"` when.
///   [`Option::None`]).
fn fzf_lua(previewer_kind: Option<String>) -> Dictionary {
    dict! {
        "previewer": previewer_kind.unwrap_or_else(|| "builtin".to_string()),
        "winopts": dict! {
            "title": "",
            "height": 1,
            "preview": dict! {
                "default":  "builtin",
                "layout":  "vertical",
                "vertical":  "down:60%",
            }
        }
    }
}
