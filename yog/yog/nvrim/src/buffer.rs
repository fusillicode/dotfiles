use nvim_oxi::Dictionary;

use crate::dict;
use crate::fn_from;

mod word_under_cursor;

/// [`Dictionary`] of buffer text helpers.
///
/// Entries:
/// - `"get_visual_selection"`: wraps [`crate::buffer::visual_selection::get`] and returns the current visual selection
///   (inclusive) as lines.
/// - `"get_word_under_cursor"`: wraps [`crate::buffer::word_under_cursor::get`] and returns a classified token under
///   the cursor.
///
/// Intended for exposure to Lua.
pub fn dict() -> Dictionary {
    dict! {
        "get_visual_selection_lines": fn_from!(crate::oxi_ext::visual_selection::get_lines),
        "get_word_under_cursor": fn_from!(word_under_cursor::get),
    }
}
