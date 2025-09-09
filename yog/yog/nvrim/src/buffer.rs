use nvim_oxi::Dictionary;

use crate::dict;
use crate::fn_from;

mod between_pos;
mod word_under_cursor;

/// [`Dictionary`] of buffer text helpers.
pub fn dict() -> Dictionary {
    dict! {
        "get_text_between_pos": fn_from!(between_pos::get),
        "get_word_under_cursor": fn_from!(word_under_cursor::get),
    }
}
