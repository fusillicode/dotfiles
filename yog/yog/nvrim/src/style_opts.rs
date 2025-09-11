use nvim_oxi::Dictionary;

use crate::dict;

/// UI style options as a Nvim [`Dictionary`].
///
/// Currently only `window.border = "rounded"`.
pub fn dict() -> Dictionary {
    dict! {
        "window": dict! {
            "border": "rounded",
        },
        "fzf_lua_previewer": dict! {
            "previewer": "builtin",
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
}
