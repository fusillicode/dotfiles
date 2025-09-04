use nvim_oxi::Dictionary;

use crate::dict;

/// Returns the desired UI style options as a Neovim [`Dictionary`].
///
/// Currently only `window.border = "rounded"`.
pub fn get(_: ()) -> Dictionary {
    dict! {
        "window": dict! {
            "border": "rounded",
        }
    }
}
