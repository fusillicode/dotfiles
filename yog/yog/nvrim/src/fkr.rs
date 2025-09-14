use fkr::FkrOption;
use nvim_oxi::Dictionary;
use nvim_oxi::api::Buffer;
use strum::IntoEnumIterator;

use crate::dict;
use crate::fn_from;
use crate::oxi_ext::buffer::BufferExt as _;

pub fn dict() -> Dictionary {
    dict! {
        "gen_string": fn_from!(gen_value),
    }
}

pub fn gen_value(_: ()) {
    let options: Vec<_> = FkrOption::iter().collect();

    let Ok(selected_option) = crate::oxi_ext::api::inputlist("Select option:", &options).inspect_err(|error| {
        crate::oxi_ext::api::notify_error(&format!("cannot get user input, error {error:#?}"));
    }) else {
        return;
    };

    if let Some(sel_opt) = selected_option {
        Buffer::current().set_text_at_cursor_pos(&sel_opt.gen_string())
    }
}
