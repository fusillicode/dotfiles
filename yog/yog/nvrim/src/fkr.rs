use fkr::FkrOption;
use nvim_oxi::Dictionary;
use nvim_oxi::api::Buffer;
use strum::IntoEnumIterator;

use crate::dict;
use crate::fn_from;
use crate::oxi_ext::buffer::BufferExt as _;

/// [`Dictionary`] of random string generation helpers powered by [`fkr`].
///
/// Entries:
/// - `"gen_string"`: wraps [`gen_value`] and inserts a generated value at the current cursor position (replacing any
///   active selection via the underlying buffer helper).
pub fn dict() -> Dictionary {
    dict! {
        "gen_string": fn_from!(gen_value),
    }
}

/// Prompt the user to select a [`fkr::FkrOption`] and insert its generated string.
///
/// The user is shown a numbered menu via [`crate::oxi_ext::api::inputlist`]; on
/// selection the corresponding generated string is inserted at the cursor using
/// [`crate::oxi_ext::buffer::BufferExt::set_text_at_cursor_pos`].
///
/// Behavior:
/// - Returns early (no insertion) if fetching user input fails or is canceled.
/// - Emits error notifications to Neovim for any underlying failures.
pub fn gen_value(_: ()) {
    let options: Vec<_> = FkrOption::iter().collect();

    let Ok(selected_option) = crate::oxi_ext::api::inputlist("Select option:", &options).inspect_err(|error| {
        crate::oxi_ext::api::notify_error(&format!("cannot get user input, error {error:#?}"));
    }) else {
        return;
    };

    if let Some(sel_opt) = selected_option {
        Buffer::current().set_text_at_cursor_pos(&sel_opt.gen_string());
    }
}
