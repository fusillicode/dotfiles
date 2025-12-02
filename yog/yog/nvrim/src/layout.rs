use nvim_oxi::Dictionary;
use nvim_oxi::api::Buffer;
use ytil_nvim_oxi::buffer::BufferExt;
use ytil_nvim_oxi::mru_buffers::BufferKind;
// use ytil_nvim_oxi::buffer::BufferExt;
// use ytil_editor::Editor;
// use ytil_editor::FileToOpen;

const TERM_WIDTH_PERC: i32 = 30;
const FILE_BUF_WIDTH_PERC: i32 = 100 - TERM_WIDTH_PERC;

/// [`Dictionary`] of Rust tests utilities.
pub fn dict() -> Dictionary {
    dict! {
        "focus_term": fn_from!(focus_term),
        "focus_buffer": fn_from!(focus_buffer),
        "smart_close_buffer": fn_from!(smart_close_buffer),
        "toggle_alternate_buffer": fn_from!(toggle_alternate_buffer),
    }
}

fn focus_term(_: ()) -> Option<()> {
    let current_buffer = nvim_oxi::api::get_current_buf();

    // If current buffer IS terminal.
    if current_buffer.is_terminal() {
        ytil_nvim_oxi::common::exec_vim_script("only", None)?;
        return Some(());
    }

    // If current buffer IS NOT terminal.
    let mut visible_windows =
        nvim_oxi::api::list_wins().map(|w| (ytil_nvim_oxi::window::get_buffer(&w).and_then(|b| b.get_buf_type()), w));

    let maybe_terminal_window = visible_windows.find(|(bt, _)| bt.as_ref().is_some_and(|b| b == "terminal"));

    // If there is a VISIBLE terminal buffer.
    if let Some((_, win)) = maybe_terminal_window {
        ytil_nvim_oxi::window::set_current(&win)?;
        ytil_nvim_oxi::common::exec_vim_script("startinsert", None)?;
        return Some(());
    }

    let width = compute_width(TERM_WIDTH_PERC)?;

    // If there is NO VISIBLE terminal buffer.
    if let Some(terminal_buffer) = nvim_oxi::api::list_bufs().find(BufferExt::is_terminal) {
        ytil_nvim_oxi::common::exec_vim_script(&format!("leftabove vsplit | vertical resize {width}"), None);
        ytil_nvim_oxi::buffer::set_current(&terminal_buffer)?;
        ytil_nvim_oxi::common::exec_vim_script("startinsert", None)?;
        return Some(());
    }

    // If there is NO terminal buffer at all.
    ytil_nvim_oxi::common::exec_vim_script(&format!("leftabove vsplit | vertical resize {width} | term"), None);
    ytil_nvim_oxi::common::exec_vim_script("startinsert", None)?;

    Some(())
}

fn focus_buffer(_: ()) -> Option<()> {
    let current_buffer = nvim_oxi::api::get_current_buf();

    // If current buffer IS NOT terminal.
    if !current_buffer.is_terminal() {
        ytil_nvim_oxi::common::exec_vim_script("only", None)?;
        return Some(());
    }

    // If current buffer IS terminal.
    let mut visible_windows =
        nvim_oxi::api::list_wins().map(|w| (ytil_nvim_oxi::window::get_buffer(&w).and_then(|b| b.get_buf_type()), w));

    let maybe_buffer_window = visible_windows.find(|(bt, _)| bt.as_ref().is_some_and(String::is_empty));

    // If there is a visible file buffer.
    if let Some((_, win)) = maybe_buffer_window {
        ytil_nvim_oxi::window::set_current(&win)?;
        return Some(());
    }

    // If there is NO visible file buffer.
    let width = compute_width(FILE_BUF_WIDTH_PERC)?;

    // Using ytil_nvim_oxi::common::exec2 because nvim_oxi::api::open_win fails with split left.
    ytil_nvim_oxi::common::exec_vim_script(&format!("vsplit | vertical resize {width}"), None)?;

    let buffer = if let Some(mru_buffer) = ytil_nvim_oxi::mru_buffers::get()?
        .iter()
        .find(|b| matches!(b.kind, BufferKind::Path | BufferKind::NoName))
    {
        Buffer::from(mru_buffer)
    } else {
        ytil_nvim_oxi::buffer::create()?
    };

    ytil_nvim_oxi::buffer::set_current(&buffer)?;

    Some(())
}

fn toggle_alternate_buffer(_: ()) -> Option<()> {
    let alt_buf_id = nvim_oxi::api::call_function::<_, i32>("bufnr", ("#",))
        .inspect_err(|err| ytil_nvim_oxi::notify::error(format!("error getting alternate buffer | err={err:?}")))
        .ok()?;

    if alt_buf_id != -1
        && let alt_buf = Buffer::from(alt_buf_id)
        && alt_buf.is_loaded()
        && !alt_buf.is_terminal()
    {
        ytil_nvim_oxi::buffer::set_current(&alt_buf)?;
        return Some(());
    }

    let current_buf = Buffer::current();
    for buf in nvim_oxi::api::list_bufs().rev() {
        if buf != current_buf
            && buf.is_loaded()
            && !buf.is_terminal()
            && buf.get_buf_type().is_some_and(|bt| bt.is_empty())
            && buf
                .get_name()
                .inspect_err(|err| {
                    ytil_nvim_oxi::notify::error(format!("error getting buffer name | buffer={buf:?} err={err:?}"));
                })
                .ok()
                .is_some_and(|bn| !bn.is_empty())
        {
            ytil_nvim_oxi::buffer::set_current(&buf)?;
            return Some(());
        }
    }

    Some(())
}

fn smart_close_buffer(force_close: Option<bool>) -> Option<()> {
    let mru_buffers = ytil_nvim_oxi::mru_buffers::get()?;

    let Some(current_buffer) = mru_buffers.first() else {
        return Some(());
    };

    let force = if force_close.is_some_and(std::convert::identity) {
        "!"
    } else {
        ""
    };

    match current_buffer.kind {
        BufferKind::Term | BufferKind::NoName => return Some(()),
        BufferKind::GrugFar => {}
        BufferKind::Path => {
            let new_current_buffer = if let Some(mru_buffer) = mru_buffers.get(1)
                && !matches!(mru_buffer.kind, BufferKind::Term)
            {
                Buffer::from(mru_buffer.id)
            } else {
                ytil_nvim_oxi::buffer::create()?
            };

            ytil_nvim_oxi::buffer::set_current(&new_current_buffer)?;
        }
    }

    ytil_nvim_oxi::common::exec_vim_script(&format!("bd{force} {}", current_buffer.id), Option::default())?;

    Some(())
}

fn compute_width(perc: i32) -> Option<i32> {
    let total_width: i32 = crate::vim_opts::get("columns", &crate::vim_opts::global_scope())?;
    Some((total_width.saturating_mul(perc)) / 100)
}

// fn open_word_under_cursor(_: ()) {
//     if !Buffer::current().is_terminal() {
//         return;
//     }
//     let Some(word_under_cursor) = crate::buffer::word_under_cursor::get(()) else {
//         return;
//     };
//     match word_under_cursor {
//         crate::buffer::word_under_cursor::WordUnderCursor::BinaryFile(_)
//         | crate::buffer::word_under_cursor::WordUnderCursor::Directory(_)
//         | crate::buffer::word_under_cursor::WordUnderCursor::Word(_) => (),
//         crate::buffer::word_under_cursor::WordUnderCursor::Url(_url) => todo!(),
//         crate::buffer::word_under_cursor::WordUnderCursor::TextFile(text_file) => {
//             Editor::Nvim.open_file_cmd(&FileToOpen {
//                 column: text_file.col,
//                 line_nbr: text_file.lnum,
//                 path: text_file.path,
//             });
//         }
//     };
// }
