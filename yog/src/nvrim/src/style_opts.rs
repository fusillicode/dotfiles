use nvim_oxi::Dictionary;

use crate::dict;

/// UI style options as a Nvim [`Dictionary`].
///
/// Currently only `window.border = "rounded"`.
pub fn dict() -> Dictionary {
    dict! {
        "window": dict! {
            "border": "rounded",
        }
    }
}
