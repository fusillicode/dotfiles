use nvim_oxi::Dictionary;

use crate::buffer::token_under_cursor;
use crate::buffer::token_under_cursor::TokenUnderCursor;

pub fn dict() -> Dictionary {
    dict! {
        "open_token_under_cursor": fn_from!(open_token_under_cursor),
    }
}

fn open_token_under_cursor(_: ()) -> Option<()> {
    let token_under_cursor = token_under_cursor::get(())?;

    match token_under_cursor {
        TokenUnderCursor::BinaryFile(_) | TokenUnderCursor::MaybeTextFile { .. } => None,
        TokenUnderCursor::TextFile { path, lnum, col } => {
            let open_path_cmd = format!(
                "edit {path} | call cursor({}, {})",
                lnum.unwrap_or_default(),
                col.unwrap_or_default()
            );

            let vim_script = if let Some(win_num) =
                ytil_noxi::window::find_with_buffer("").and_then(|(win, _)| ytil_noxi::window::get_number(&win))
            {
                format!("{win_num} wincmd w | {open_path_cmd}")
            } else {
                let width = crate::layout::compute_width(70)?;
                format!("vsplit | vertical resize {width} | {open_path_cmd}")
            };

            ytil_noxi::common::exec_vim_script(&vim_script, None);

            Some(())
        }
        TokenUnderCursor::Url(arg) | TokenUnderCursor::Directory(arg) => ytil_sys::open(&arg)
            .inspect_err(|err| {
                ytil_noxi::notify::error(format!("{err:?}"));
            })
            .ok(),
    }
}
