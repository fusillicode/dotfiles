//! Buffer text extraction helpers exposed to Lua.
//!
//! Provides a dictionary with functions to obtain visual selection lines and classify the word under
//! cursor (see `token_under_cursor`).

use nvim_oxi::Dictionary;

pub mod token_under_cursor;

/// [`Dictionary`] of buffer text helpers.
///
/// Entries:
/// - `"get_visual_selection"`: wraps [`ytil_noxi::visual_selection::get_lines`] and returns the current visual
///   selection (inclusive) as lines.
/// - `"get_selection_for_ex_range"`: returns selected text plus 0-based buffer coordinates for an Ex range.
/// - `"get_marked_visual_selection"`: returns persisted visual text plus 0-based buffer coordinates.
/// - `"get_visual_range_command_prefix"`: returns the Ex range prefix for a persisted visual range.
/// - `"get_token_under_cursor"`: wraps [`crate::buffer::token_under_cursor::get`] and returns a classified token under
///   the cursor.
///
/// Intended for exposure to Lua.
pub fn dict() -> Dictionary {
    dict! {
        "get_marked_visual_selection": fn_from!(ytil_noxi::visual_selection::get_marked),
        "get_selection_for_ex_range": fn_from!(ytil_noxi::visual_selection::get_for_ex_range),
        "get_visual_range_command_prefix": fn_from!(ytil_noxi::visual_selection::get_visual_range_command_prefix),
        "get_visual_selection_lines": fn_from!(ytil_noxi::visual_selection::get_lines),
        "get_token_under_cursor": fn_from!(token_under_cursor::get),
    }
}
