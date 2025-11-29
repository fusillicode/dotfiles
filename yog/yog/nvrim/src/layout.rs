#![allow(dead_code)]

use nvim_oxi::Array;
use nvim_oxi::Dictionary;
use nvim_oxi::Object;
use nvim_oxi::api::Buffer;
use nvim_oxi::api::Window;
use nvim_oxi::api::opts::ExecOpts;
use nvim_oxi::conversion::FromObject;
use nvim_oxi::lua::Poppable;
use nvim_oxi::lua::ffi::State;
use serde::Deserialize;
use ytil_nvim_oxi::buffer::BufferExt;
// use ytil_nvim_oxi::buffer::BufferExt;
// use ytil_editor::Editor;
// use ytil_editor::FileToOpen;

/// [`Dictionary`] of Rust tests utilities.
pub fn dict() -> Dictionary {
    dict! {
        "focus_term": fn_from!(focus_term),
        "focus_buffer": fn_from!(focus_buffer),
        "ga": fn_from!(ga),
    }
}

fn focus_term(_: ()) -> Option<()> {
    let current_buffer = nvim_oxi::api::get_current_buf();

    // If current buffer is terminal and not full screen make it full screen.
    if current_buffer.is_terminal() {
        if nvim_oxi::api::list_wins().len() != 1 {
            exec2("only", None)?;
        }
        return Some(());
    }

    // If current buffer is not terminal and full screen focus the terminal buffer or create a new
    // one if not found.
    let visible_windows = nvim_oxi::api::list_wins();
    if visible_windows.len() == 1 {
        let width = compute_width(TERM_WIDTH_PERC)?;

        // Using exec2 because nvim_oxi::api::open_win fails with split left.
        if let Some(terminal_buffer) = nvim_oxi::api::list_bufs().find(BufferExt::is_terminal) {
            exec2(&format!("leftabove vsplit | vertical resize {width}"), None);
            set_current_buf(&terminal_buffer)?;
        } else {
            exec2(&format!("leftabove vsplit | vertical resize {width} | term"), None);
        }

        // Cannot chain "startinsert" in previous exec2 because of this error:
        // ```
        // zsh:1: parse error near `|'
        //
        // [Process exited 1]
        // ````
        exec2("startinsert", None)?;
        return Some(());
    }

    // If current buffer is not terminal and not full screen focus the terminal buffer.
    for win in visible_windows {
        if get_window_buf(&win)?.is_terminal() {
            nvim_oxi::api::set_current_win(&win)
                .inspect_err(|err| {
                    ytil_nvim_oxi::notify::error(format!("error setting current window | window={win:?}, err={err:?}"))
                })
                .ok()?;
            exec2("startinsert", None)?;
            // Exit as soon as the first terminal is found.
            return Some(());
        }
    }

    Some(())
}

fn focus_buffer(_: ()) -> Option<()> {
    let current_buffer = nvim_oxi::api::get_current_buf();

    // If current buffer is terminal.
    if current_buffer.is_terminal() {
        let visible_windows = nvim_oxi::api::list_wins();

        // Terminal is full screen.
        if visible_windows.len() == 1 {
            let width = compute_width(FILE_BUF_WIDTH_PERC)?;

            // Using exec2 because nvim_oxi::api::open_win fails with split left.
            exec2(&format!("vsplit | vertical resize {width}"), None)?;

            let Ok(last_buffer_id) = nvim_oxi::api::call_function::<_, i32>("bufnr", ("#",)) else {
                return None;
            };

            let buffer = if last_buffer_id < 0 {
                nvim_oxi::api::create_buf(true, false)
                    .inspect_err(|err| ytil_nvim_oxi::notify::error(format!("error creating buffer | err={err:?}")))
                    .ok()?
            } else {
                Buffer::from(last_buffer_id)
            };

            set_current_buf(&buffer)?;

        // Terminal is NOT full screen.
        } else {
            for win in visible_windows {
                if get_window_buf(&win)?.get_buf_type().is_some_and(|bt| bt.is_empty()) {
                    nvim_oxi::api::set_current_win(&win)
                        .inspect_err(|err| {
                            ytil_nvim_oxi::notify::error(format!(
                                "error setting current window | window={win:?}, err={err:?}"
                            ))
                        })
                        .ok()?;
                }
            }
        }
    // Current buffer IS NOT terminal.
    } else {
        // Not terminal is not full screen.
        if nvim_oxi::api::list_wins().len() != 1 {
            exec2("only", None)?;
        }
    }

    Some(())
}

#[derive(Debug, Deserialize)]
#[allow(unused)]
struct JumpList(Vec<JumpEntry>, usize);

#[derive(Clone, Debug, Deserialize)]
pub struct JumpEntry {
    pub bufnr: i32,
    pub col: i32,
    pub coladd: i32,
    pub lnum: i32,
}

impl FromObject for JumpList {
    fn from_object(obj: Object) -> Result<Self, nvim_oxi::conversion::Error> {
        Self::deserialize(nvim_oxi::serde::Deserializer::new(obj)).map_err(Into::into)
    }
}

impl Poppable for JumpList {
    unsafe fn pop(lstate: *mut State) -> Result<Self, nvim_oxi::lua::Error> {
        unsafe {
            let obj = Object::pop(lstate)?;
            Self::from_object(obj).map_err(nvim_oxi::lua::Error::pop_error_from_err::<Self, _>)
        }
    }
}

fn get_jumplist() -> Option<Vec<JumpEntry>> {
    Some(
        nvim_oxi::api::call_function::<_, JumpList>("getjumplist", Array::new())
            .inspect_err(|err| ytil_nvim_oxi::notify::error(format!("error getting jumplist | err={err:?}")))
            .ok()?
            .0,
    )
}

fn ga(_: ()) -> Option<()> {
    let alt_buf_id = nvim_oxi::api::call_function::<_, i32>("bufnr", ("#",))
        .inspect_err(|err| ytil_nvim_oxi::notify::error(format!("error getting alternate buffer | err={err:?}")))
        .ok()?;

    if alt_buf_id != -1
        && let alt_buf = Buffer::from(alt_buf_id)
        && alt_buf.is_loaded()
        && !alt_buf.is_terminal()
    {
        set_current_buf(&alt_buf)?;
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
                    ytil_nvim_oxi::notify::error(format!("error getting buffer name | buffer={buf:?} err={err:?}"))
                })
                .ok()
                .is_some_and(|bn| !bn.is_empty())
        {
            set_current_buf(&buf)?;
            return Some(());
        }
    }

    Some(())
}

fn set_current_buf(buf: &Buffer) -> Option<()> {
    nvim_oxi::api::set_current_buf(buf)
        .inspect_err(|err| {
            ytil_nvim_oxi::notify::error(format!("error setting current buffer | buffer={buf:?} err={err:?}"))
        })
        .ok()?;
    Some(())
}

fn get_window_buf(win: &Window) -> Option<Buffer> {
    win.get_buf()
        .inspect_err(|err| {
            ytil_nvim_oxi::notify::error(format!("error getting window buffer | window={win:?}, err={err:?}"))
        })
        .ok()
}

const TERM_WIDTH_PERC: i32 = 30;
const FILE_BUF_WIDTH_PERC: i32 = 100 - TERM_WIDTH_PERC;

fn compute_width(perc: i32) -> Option<i32> {
    let total_width: i32 = crate::vim_opts::get("columns", &crate::vim_opts::global_scope())?;
    Some((total_width * perc) / 100)
}

// Option<Option> to be able to use ? and short circuit.
#[allow(clippy::option_option)]
fn exec2(src: &str, opts: Option<ExecOpts>) -> Option<Option<String>> {
    let opts = opts.unwrap_or_default();
    Some(
        nvim_oxi::api::exec2(src, &opts)
            .inspect_err(|err| {
                ytil_nvim_oxi::notify::error(format!(
                    "error executing Vimscript | src={src:?} opts={opts:?} err={err:?}"
                ))
            })
            .ok()?
            .map(|s| s.to_string()),
    )
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
