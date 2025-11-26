use nvim_oxi::Dictionary;
use nvim_oxi::api::Window;
use nvim_oxi::api::opts::CreateAutocmdOptsBuilder;
use nvim_oxi::api::opts::ExecOptsBuilder;
use ytil_nvim_oxi::buffer::BufferExt;

/// [`Dictionary`] of Rust tests utilities.
pub fn dict() -> Dictionary {
    dict! {
        "toggle_term": fn_from!(toggle_term),
    }
}

pub fn create_autocmd() {
    crate::cmds::create_autocmd(
        ["BufEnter"],
        "EnterTerminal",
        CreateAutocmdOptsBuilder::default()
            .patterns(["term://*"])
            .command("startinsert"),
    );
}

fn toggle_term(_: ()) {
    let Some(terminal_buffer) = nvim_oxi::api::list_bufs()
        .into_iter()
        .find(BufferExt::is_terminal_buffer)
    else {
        new_term(30);
        return;
    };

    let Some(visible_terminal_win) = nvim_oxi::api::list_wins().into_iter().find(|win| {
        if let Ok(buffer) = win.get_buf().inspect_err(|err| {
            ytil_nvim_oxi::notify::error(format!(
                "error getting buffer from window | window={win:?} error={err:?}",
            ));
        }) {
            buffer.is_terminal_buffer()
        } else {
            false
        }
    }) else {
        let Some(total_width): Option<u32> = crate::vim_opts::get("columns", &crate::vim_opts::global_scope()) else {
            return;
        };

        let term_width = (total_width * 30) / 100;

        // Terminal exists but hidden: show it
        if let Err(err) = nvim_oxi::api::exec2("leftabove vsplit", &ExecOptsBuilder::default().build()) {
            ytil_nvim_oxi::notify::error(format!("error executing vim cmd | width={term_width:#?} error={err:?}",));
            return;
        };

        if let Err(err) = nvim_oxi::api::set_current_buf(&terminal_buffer) {
            ytil_nvim_oxi::notify::error(format!("error executing vim cmd | width={term_width:#?} error={err:?}",));
            return;
        }

        let mut current_window = Window::current();
        if let Err(err) = current_window.set_width(term_width) {
            ytil_nvim_oxi::notify::error(format!(
                "error setting width of current window | current_window={current_window:?} width={term_width:#?} error={err:?}",
            ));
        }
        return;
    };

    // Terminal is visible: close it
    if let Err(err) = visible_terminal_win.clone().close(false) {
        ytil_nvim_oxi::notify::error(format!(
            "error closing window | window={visible_terminal_win:?} error={err:?}",
        ));
    }
}

fn new_term(width_perc: u32) {
    let Some(total_width): Option<u32> = crate::vim_opts::get("columns", &crate::vim_opts::global_scope()) else {
        return;
    };

    let term_width = (total_width * width_perc) / 100;
    if let Err(err) = nvim_oxi::api::exec2("leftabove vsplit", &ExecOptsBuilder::default().build()) {
        ytil_nvim_oxi::notify::error(format!("error executing vim cmd | width={term_width:#?} error={err:?}",));
    };
    let _ = ytil_nvim_oxi::common::exec_vim_cmd("terminal", None::<&[&str]>);

    let mut current_window = Window::current();
    if let Err(err) = current_window.set_width(term_width) {
        ytil_nvim_oxi::notify::error(format!(
            "error setting width of current window | current_window={current_window:?} width={term_width:#?} error={err:?}",
        ));
    }
}
