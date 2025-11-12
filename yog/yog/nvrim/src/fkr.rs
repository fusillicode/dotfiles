//! Random string generation helpers backed by [`fkr`].
//!
//! Exposes a dictionary with an insertion command (`insert_string`) prompting the user to select an
//! [`::fkr::FkrOption`] then inserting the generated string at the cursor. Input / buffer errors are
//! reported via [`ytil_nvim_oxi::api::notify_error`].

use fkr::FkrOption;
use nvim_oxi::Dictionary;
use nvim_oxi::api::Buffer;
use strum::IntoEnumIterator;
use ytil_nvim_oxi::buffer::BufferExt as _;

/// [`Dictionary`] of random string generation helpers powered by [`fkr`].
///
/// Entries:
/// - `"insert_string"`: inserts a generated value at the current cursor position replacing any active selection via the
///   buffer helper.
pub fn dict() -> Dictionary {
    dict! {
        "insert_string": fn_from!(insert_string),
    }
}

/// Prompt the user to select a [`fkr::FkrOption`] and insert its generated string.
///
/// The user is shown a numbered menu via [`ytil_nvim_oxi::api::inputlist`]; on
/// selection the corresponding generated string is inserted at the cursor using
/// [`ytil_nvim_oxi::buffer::BufferExt::set_text_at_cursor_pos`].
///
/// Behaviour:
/// - Returns early (no insertion) if fetching user input fails or is canceled.
/// - Emits error notifications to Nvim for selection prompt or buffer write failures.
fn insert_string(_: ()) {
    let options: Vec<_> = FkrOption::iter().collect();

    let Ok(selected_option) = ytil_nvim_oxi::api::inputlist("Select option:", &options).inspect_err(|error| {
        ytil_nvim_oxi::api::notify_error(format!("cannot get user input | error={error:#?}"));
    }) else {
        return;
    };

    if let Some(sel_opt) = selected_option {
        Buffer::current().set_text_at_cursor_pos(&sel_opt.gen_string());
    }
}
