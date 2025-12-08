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
        WordUnderCursor::TextFile(text_file) => None,
        WordUnderCursor::Url(arg) | WordUnderCursor::Directory(arg) => ytil_sys::open(arg).inspect_err(|err| {
            ytil_noxi::notify::error(format!("{error:?}"));
        }),
    }
}
