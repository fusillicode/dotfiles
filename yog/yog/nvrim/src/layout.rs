use nvim_oxi::Dictionary;
use nvim_oxi::api::Buffer;
use nvim_oxi::api::opts::CreateAutocmdOpts;
use ytil_noxi::buffer::BufferExt;
use ytil_noxi::mru_buffers::BufferKind;
use ytil_noxi::mru_buffers::MruBuffer;

/// [`Dictionary`] of Rust tests utilities.
pub fn dict() -> Dictionary {
    dict! {
        "focus_term": fn_from!(focus_term),
        "focus_buffer": fn_from!(focus_buffer),
        "smart_close_buffer": fn_from!(smart_close_buffer),
        "toggle_alternate_buffer": fn_from!(toggle_alternate_buffer),
    }
}

pub fn create_autocmd() {
    crate::cmds::create_autocmd(
        ["BufEnter", "WinEnter", "TermOpen"],
        "TerminalAutoInsertMode",
        CreateAutocmdOpts::builder()
            .patterns(["term://*"])
            .command("startinsert"),
    );
}

fn focus_term(width_perc: i32) -> Option<()> {
    let current_buffer = nvim_oxi::api::get_current_buf();

    // If current buffer IS terminal.
    if current_buffer.is_terminal() {
        ytil_noxi::common::exec_vim_script("only", None)?;
        return Some(());
    }

    // If current buffer IS NOT terminal.
    let maybe_terminal_window = ytil_noxi::window::find_with_buffer("terminal");

    // If there is a VISIBLE terminal buffer.
    if let Some((win, _)) = maybe_terminal_window {
        ytil_noxi::window::set_current(&win)?;
        return Some(());
    }

    let width = compute_width(width_perc)?;

    // If there is a NON-VISIBLE listed terminal buffer.
    // Uses mru_buffers::get() ("ls t") which only returns listed buffers,
    // excluding unlisted plugin UI terminals (e.g. fzf-lua).
    if let Some(mru_term) = ytil_noxi::mru_buffers::get().and_then(|bufs| bufs.into_iter().find(MruBuffer::is_term)) {
        ytil_noxi::common::exec_vim_script(&format!("leftabove vsplit | vertical resize {width}"), None);
        ytil_noxi::buffer::set_current(&Buffer::from(&mru_term))?;
        return Some(());
    }

    // If there is NO terminal buffer at all.
    ytil_noxi::common::exec_vim_script(&format!("leftabove vsplit | vertical resize {width} | term"), None);

    Some(())
}

fn focus_buffer(width_perc: i32) -> Option<()> {
    let current_buffer = nvim_oxi::api::get_current_buf();

    // If current buffer IS NOT terminal.
    if !current_buffer.is_terminal() {
        ytil_noxi::common::exec_vim_script("only", None)?;
        return Some(());
    }

    // If current buffer IS terminal.
    let maybe_buffer_window = ytil_noxi::window::find_with_buffer("");

    // If there is a visible file buffer.
    if let Some((win, _)) = maybe_buffer_window {
        ytil_noxi::window::set_current(&win)?;
        return Some(());
    }

    // If there is NO visible file buffer.
    let width = compute_width(width_perc)?;

    // Using ytil_noxi::common::exec2 because nvim_oxi::api::open_win fails with split left.
    ytil_noxi::common::exec_vim_script(&format!("vsplit | vertical resize {width}"), None)?;

    let buffer = if let Some(mru_buffer) = ytil_noxi::mru_buffers::get()?
        .iter()
        .find(|b| matches!(b.kind, BufferKind::Path | BufferKind::NoName))
    {
        Buffer::from(mru_buffer)
    } else {
        ytil_noxi::buffer::create()?
    };

    ytil_noxi::buffer::set_current(&buffer)?;

    Some(())
}

fn toggle_alternate_buffer(_: ()) -> Option<()> {
    let alt_buf_id = nvim_oxi::api::call_function::<_, i32>("bufnr", ("#",))
        .inspect_err(|err| ytil_noxi::notify::error(format!("error getting alternate buffer | error={err:?}")))
        .ok()?;

    if alt_buf_id != -1
        && let alt_buf = Buffer::from(alt_buf_id)
        && alt_buf.is_loaded()
        && !alt_buf.is_terminal()
    {
        ytil_noxi::buffer::set_current(&alt_buf)?;
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
                    ytil_noxi::notify::error(format!("error getting buffer name | buffer={buf:?} error={err:?}"));
                })
                .ok()
                .is_some_and(|bn| !bn.is_empty())
        {
            ytil_noxi::buffer::set_current(&buf)?;
            return Some(());
        }
    }

    Some(())
}

fn smart_close_buffer(force_close: Option<bool>) -> Option<()> {
    let mru_buffers = ytil_noxi::mru_buffers::get()?;

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
                ytil_noxi::buffer::create()?
            };

            ytil_noxi::buffer::set_current(&new_current_buffer)?;
        }
    }

    ytil_noxi::common::exec_vim_script(&format!("bd{force} {}", current_buffer.id), Option::default())?;

    Some(())
}

pub fn compute_width(perc: i32) -> Option<i32> {
    let total_width: i32 = crate::vim_opts::get("columns", &crate::vim_opts::global_scope())?;
    Some((total_width.saturating_mul(perc)) / 100)
}
