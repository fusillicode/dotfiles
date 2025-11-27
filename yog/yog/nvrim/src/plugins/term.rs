use nvim_oxi::Dictionary;
use nvim_oxi::api::Buffer;
use nvim_oxi::api::Window;
use nvim_oxi::api::opts::CreateAutocmdOptsBuilder;
use nvim_oxi::api::opts::ExecOptsBuilder;
// use ytil_editor::Editor;
// use ytil_editor::FileToOpen;
use ytil_nvim_oxi::buffer::BufferExt;

/// [`Dictionary`] of Rust tests utilities.
pub fn dict() -> Dictionary {
    dict! {
        "toggle": fn_from!(toggle),
        "toggle_term_modes": fn_from!(toggle_term_modes),
        // "open_word_under_cursor": fn_from!(open_word_under_cursor),
    }
}

pub fn create_autocmd() {
    crate::cmds::create_autocmd(
        ["BufEnter", "TermOpen"],
        "EnterTerminal",
        CreateAutocmdOptsBuilder::default()
            .patterns(["term://*"])
            .command("startinsert"),
    );
}

fn toggle_term_modes(_: ()) {
    let current_buffer = Buffer::current();
    if !current_buffer.is_terminal() {
        return;
    }
    if nvim_oxi::api::get_mode().mode != "i" {
        let _ = ytil_nvim_oxi::common::exec_vim_cmd("startinsert", None::<&[&str]>);
    }
}

fn toggle(width_perc: u32) {
    let Some(terminal_buffer) = nvim_oxi::api::list_bufs().into_iter().find(BufferExt::is_terminal) else {
        create_or_show_terminal_buffer(width_perc, TerminalBufferOp::Create);
        return;
    };

    let Some(visible_terminal_window) = nvim_oxi::api::list_wins().into_iter().find(|win| {
        if let Ok(buffer) = win.get_buf().inspect_err(|err| {
            ytil_nvim_oxi::notify::error(format!(
                "error getting buffer from window | window={win:?} error={err:?}",
            ));
        }) {
            buffer.is_terminal()
        } else {
            false
        }
    }) else {
        // If terminal buffer is not visible, show it.
        create_or_show_terminal_buffer(width_perc, TerminalBufferOp::Show(&terminal_buffer));
        return;
    };

    // If terminal buffer is visible, hide it.
    if let Err(err) = visible_terminal_window.clone().close(false) {
        ytil_nvim_oxi::notify::error(format!(
            "error closing window | window={visible_terminal_window:?} error={err:?}",
        ));
    }
}

#[derive(Clone, Copy)]
enum TerminalBufferOp<'a> {
    Create,
    Show(&'a Buffer),
}

impl<'a> TerminalBufferOp<'a> {
    pub fn run(&'a self) {
        match self {
            TerminalBufferOp::Create => {
                // Error already notified internally by [`exec_vim_cmd`].
                let _ = ytil_nvim_oxi::common::exec_vim_cmd("terminal", None::<&[&str]>);
            }
            TerminalBufferOp::Show(buffer) => {
                let _ = nvim_oxi::api::set_current_buf(buffer).inspect_err(|err| {
                    ytil_nvim_oxi::notify::error(format!(
                        "error setting buffer as current | buffer={buffer:?} error={err:?}"
                    ));
                });
            }
        }
    }
}

fn create_or_show_terminal_buffer(width_perc: u32, op: TerminalBufferOp) {
    // Error already notified internally by [`crate::vim_opts::get`].
    let Some(total_width): Option<u32> = crate::vim_opts::get("columns", &crate::vim_opts::global_scope()) else {
        return;
    };

    if let Err(err) = nvim_oxi::api::exec2("leftabove vsplit", &ExecOptsBuilder::default().build()) {
        ytil_nvim_oxi::notify::error(format!(
            "error vsplitting buffer | width_perc={width_perc} error={err:?}"
        ));
    };

    op.run();

    let term_width = (total_width * width_perc) / 100;
    let mut current_window = Window::current();
    if let Err(err) = current_window.set_width(term_width) {
        ytil_nvim_oxi::notify::error(format!(
            "error setting width of current window | current_window={current_window:?} width={term_width:#?} error={err:?}",
        ));
    }
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
