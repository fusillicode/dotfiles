use nvim_oxi::Dictionary;

use crate::dict;
use crate::fn_from;

/// UI style options as a Nvim [`Dictionary`].
///
/// Currently only `window.border = "rounded"`.
pub fn dict() -> Dictionary {
    dict! {
        "window": dict! {
            "border": "rounded",
        },
        "fzf_lua_previewer": fn_from!(fzf_lua_previewer)
    }
}

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
