//! Random string generation helpers backed by [`fkr`].
//!
//! Exposes a dictionary with an insertion command (`insert_string`) prompting the user to select an
//! [`::fkr::FkrOption`] then inserting the generated string at the cursor. Input / buffer errors are
//! reported via [`ytil_noxi::notify::error`].

use fkr::FkrOption;
use nvim_oxi::Dictionary;
use nvim_oxi::api::Buffer;
use strum::IntoEnumIterator;
use ytil_noxi::buffer::BufferExt as _;

/// [`Dictionary`] of random string generation helpers powered by [`fkr`].
///
/// Entries:
/// - `"insert_string"` inserts a generated value at the current cursor position replacing any active selection via the
///   buffer helper.
pub fn dict() -> Dictionary {
    dict! {
        "insert_string": fn_from!(insert_string),
    }
}

/// Prompt the user to select a [`fkr::FkrOption`] and insert its generated string.
///
/// The user is shown a selection menu via [`ytil_noxi::vim_ui_select::open`]; on
/// selection the corresponding generated string is inserted at the cursor using
/// [`ytil_noxi::buffer::BufferExt::set_text_at_cursor_pos`].
///
/// Behaviour:
/// - Returns early (no insertion) if fetching user input fails or is canceled.
/// - Emits error notifications to Nvim for selection prompt or buffer write failures.
fn insert_string(_: ()) {
    let opts: Vec<FkrOption> = FkrOption::iter().collect();

    let callback = {
        let opts = opts.clone();
        move |choice_idx| {
            let selected_opt: Option<&FkrOption> = opts.get(choice_idx);
            if let Some(selected_opt) = selected_opt {
                Buffer::current().set_text_at_cursor_pos(&selected_opt.gen_string());
            }
        }
    };

    if let Err(err) = ytil_noxi::vim_ui_select::open(opts, &[("prompt", "Select option: ")], callback, None) {
        ytil_noxi::notify::error(format!("error generating fkr value | error={err:#?}"));
    }
}
