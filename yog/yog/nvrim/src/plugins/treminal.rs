use nvim_oxi::Dictionary;
use nvim_oxi::api::Window;
use nvim_oxi::api::opts::CreateAutocmdOptsBuilder;
use nvim_oxi::api::opts::ExecOptsBuilder;
use nvim_oxi::api::types::AutocmdCallbackArgs;
use ytil_nvim_oxi::buffer::BufferExt;

/// [`Dictionary`] of Rust tests utilities.
pub fn dict() -> Dictionary {
    dict! {
        "toggle_term": fn_from!(toggle_term),
    }
}

pub fn create_autocmd() {
    crate::cmds::create_autocmd(
        ["WinEnter", "BufWinEnter", "TermOpen"],
        "EnterTerminal",
        // TODO: Can `.patterns("term://")` be used?
        CreateAutocmdOptsBuilder::default().callback(|args: AutocmdCallbackArgs| {
            if let Ok(buffer_name) = args.buffer.get_name()
                && buffer_name.to_string_lossy().starts_with("term://")
            {
                let _ = ytil_nvim_oxi::common::exec_vim_cmd("startinsert", None::<&[&str]>);
            }
            true
        }),
    );
}

fn toggle_term(_: ()) {
    let buffers = nvim_oxi::api::list_bufs();

    let Some(terminal_buffer) = buffers
        .into_iter()
        .find(|b| b.get_buf_type().unwrap_or_default() == "terminal")
    else {
        create_term();
        return;
    };

    let windows = nvim_oxi::api::list_wins();
    let Some(visible_terminal_win) = windows.into_iter().find(|win| {
        if let Ok(buffer) = win.get_buf() {
            buffer.get_buf_type().unwrap_or_default() == "terminal"
        } else {
            false
        }
    }) else {
        // Terminal exists but hidden: show it
        nvim_oxi::api::command(&format!("split | buffer {}", terminal_buffer.handle())).unwrap();
        let _ = ytil_nvim_oxi::common::exec_vim_cmd("startinsert", None::<&[&str]>);
        return;
    };

    // Terminal is visible: close it
    visible_terminal_win.close(false).unwrap();
}

fn create_term() {
    let Some(total_width): Option<u32> = crate::vim_opts::get("columns", &crate::vim_opts::global_scope()) else {
        return;
    };

    let term_width = (total_width * 30) / 100;
    if let Err(err) = nvim_oxi::api::exec2("leftabove vsplit", &ExecOptsBuilder::default().build()) {
        ytil_nvim_oxi::notify::error(format!("error executing vim cmd | width={term_width:#?} error={err:?}",));
    };
    let _ = ytil_nvim_oxi::common::exec_vim_cmd("terminal", None::<&[&str]>);
    let _ = ytil_nvim_oxi::common::exec_vim_cmd("startinsert", None::<&[&str]>);

    let mut current_window = Window::current();
    if let Err(err) = current_window.set_width(term_width) {
        ytil_nvim_oxi::notify::error(format!(
            "error setting width of current window | current_window={current_window:?} width={term_width:#?} error={err:?}",
        ));
    }
}
