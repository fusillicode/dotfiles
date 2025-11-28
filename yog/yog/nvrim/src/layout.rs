#![allow(unused_imports, dead_code, clippy::needless_return, unused_variables)]

use nvim_oxi::Dictionary;
use nvim_oxi::api::Buffer;
use nvim_oxi::api::opts::ExecOpts;
use nvim_oxi::api::types::SplitDirection;
use nvim_oxi::api::types::WindowConfigBuilder;
use ytil_nvim_oxi::buffer::BufferExt;
// use ytil_nvim_oxi::buffer::BufferExt;
// use ytil_editor::Editor;
// use ytil_editor::FileToOpen;

/// [`Dictionary`] of Rust tests utilities.
pub fn dict() -> Dictionary {
    dict! {
        "focus_term": fn_from!(focus_term),
        "focus_buffer": fn_from!(focus_buffer),
    }
}

// fn setup_layout(_: ()) {
//     nvim_oxi::api::command("vsplit").unwrap();
//     nvim_oxi::api::command("terminal").unwrap();
//     nvim_oxi::api::command("wincmd h").unwrap();
// }
//

pub fn focus_term(_: ()) -> Option<()> {
    let current_buffer = nvim_oxi::api::get_current_buf();

    // If current buffer is NOT terminal.
    if !current_buffer.is_terminal() {
        let visible_windows = nvim_oxi::api::list_wins();

        // Current buffer is full screen.
        if visible_windows.len() == 1 {
            // Get current total columns in the editor
            let total_cols: i32 = crate::vim_opts::get("columns", &crate::vim_opts::global_scope())?;

            // Calculate 30% width
            let width = (total_cols as f64 * 0.3).round() as u32;

            // Using exec2 because nvim_oxi::api::open_win fails with split left.
            exec2(
                &format!("leftabove vsplit | vertical resize {width} | term"),
                &Default::default(),
            );

            // Cannot chain "startinsert" in previous exec2 because of this error:
            // ```
            // zsh:1: parse error near `|'
            //
            // [Process exited 1]
            // ````
            exec2("startinsert", &Default::default())?;

        // Current buffer is NOT full screen.
        } else {
            for win in visible_windows {
                if win
                    .get_buf()
                    .inspect_err(|err| {
                        ytil_nvim_oxi::notify::error(format!(
                            "error getting window buffer | window={win:?}, err={err:?}"
                        ))
                    })
                    .ok()?
                    .is_terminal()
                {
                    nvim_oxi::api::set_current_win(&win)
                        .inspect_err(|err| {
                            ytil_nvim_oxi::notify::error(format!(
                                "error setting current window | window={win:?}, err={err:?}"
                            ))
                        })
                        .ok()?;
                    exec2("startinsert", &Default::default())?;
                }
            }
        }
    // Current buffer IS TERMINAL
    } else {
        // Current buffer is NOT full screen.
        if nvim_oxi::api::list_wins().len() != 1 {
            exec2("only", &Default::default())?;
        }
    }

    Some(())
}

pub fn focus_buffer(_: ()) -> Option<()> {
    let current_buffer = nvim_oxi::api::get_current_buf();

    // If current buffer is terminal.
    if current_buffer.is_terminal() {
        let visible_windows = nvim_oxi::api::list_wins();

        // Current buffer is full screen.
        if visible_windows.len() == 1 {
            // Get current total columns in the editor
            let total_cols: i32 = crate::vim_opts::get("columns", &crate::vim_opts::global_scope())?;

            // Calculate 70% width
            let width = (total_cols as f64 * 0.7).round() as u32;

            // Using exec2 because nvim_oxi::api::open_win fails with split left.
            exec2(&format!("vsplit | vertical resize {width}"), &Default::default())?;

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

            nvim_oxi::api::set_current_buf(&buffer)
                .inspect_err(|err| {
                    ytil_nvim_oxi::notify::error(format!(
                        "error setting current buffer | buffer={buffer:?}, err={err:?}"
                    ))
                })
                .ok()?;

        // Current buffer is NOT full screen.
        } else {
            for win in visible_windows {
                if !win
                    .get_buf()
                    .inspect_err(|err| {
                        ytil_nvim_oxi::notify::error(format!(
                            "error getting window buffer | window={win:?}, err={err:?}"
                        ))
                    })
                    .ok()?
                    .is_terminal()
                {
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
        // Current buffer is not full screen.
        if nvim_oxi::api::list_wins().len() != 1 {
            exec2("only", &Default::default())?;
        }
    }

    Some(())
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

fn exec2(src: &str, opts: &ExecOpts) -> Option<Option<String>> {
    Some(
        nvim_oxi::api::exec2(src, opts)
            .inspect_err(|err| {
                ytil_nvim_oxi::notify::error(format!("error executing Vimscript | src={src:?}, opts={opts:?}"))
            })
            .ok()?
            .map(|s| s.to_string()),
    )
}
