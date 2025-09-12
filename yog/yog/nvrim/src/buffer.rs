use nvim_oxi::Dictionary;

use crate::dict;
use crate::fn_from;

mod visual_selection;
mod word_under_cursor;

/// [`Dictionary`] of buffer text helpers.
pub fn dict() -> Dictionary {
    dict! {
        "get_visual_selection": fn_from!(visual_selection::get),
        "get_word_under_cursor": fn_from!(word_under_cursor::get),
    }
}
