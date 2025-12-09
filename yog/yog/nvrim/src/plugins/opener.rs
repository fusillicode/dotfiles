use nvim_oxi::Dictionary;

use crate::buffer::word_under_cursor;
use crate::buffer::word_under_cursor::WordUnderCursor;

pub fn dict() -> Dictionary {
    dict! {
        "open_word_under_cursor": fn_from!(open_word_under_cursor),
    }
}

fn open_word_under_cursor(_: ()) -> Option<()> {
    let word_under_cursor = word_under_cursor::get(())?;

    match word_under_cursor {
        WordUnderCursor::BinaryFile(_) | WordUnderCursor::Word(_) => None,
        WordUnderCursor::TextFile { path, lnum, .. } => {
            let open_path_at_line_cmd = format!("edit +{lnum} {path}");
            let vim_script = if let Some(win_num) =
                ytil_noxi::window::find_window_with_buffer("").and_then(|(win, _)| ytil_noxi::window::get_number(&win))
            {
                format!("{win_num} wincmd w | {open_path_at_line_cmd}")
            } else {
                let width = crate::layout::compute_width(70)?;
                format!("vsplit | vertical resize {width} | {open_path_at_line_cmd}")
            };
            ytil_noxi::common::exec_vim_script(&vim_script, None);
            Some(())
        }
        WordUnderCursor::Url(arg) | WordUnderCursor::Directory(arg) => ytil_sys::open(&arg)
            .inspect_err(|err| {
                ytil_noxi::notify::error(format!("{err:?}"));
            })
            .ok(),
    }
}
