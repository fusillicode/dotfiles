use nvim_oxi::Dictionary;
use nvim_oxi::api::Buffer;
use nvim_oxi::api::opts::CreateAutocmdOpts;
use serde::Deserialize;
use ytil_noxi::mru_buffers::BufferKind;

/// [`Dictionary`] of Rust tests utilities.
pub fn dict() -> Dictionary {
    dict! {
        "focus_vsplit": fn_from!(focus_vsplit),
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

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all(deserialize = "snake_case"))]
pub enum SplitKind {
    Term,
    Buffer,
}

impl SplitKind {
    pub const fn is_term(self) -> bool {
        match self {
            Self::Term => true,
            Self::Buffer => false,
        }
    }
}

ytil_noxi::impl_nvim_deserializable!(SplitKind);

fn focus_vsplit((split_kind, width_perc): (SplitKind, i32)) -> Option<()> {
    // Single MRU fetch - source of truth for all terminal buffer lookups.
    let mru_bufs = ytil_noxi::mru_buffers::get().unwrap_or_default();
    let term_bufs: Vec<Buffer> = mru_bufs.iter().filter(|b| b.is_term()).map(Buffer::from).collect();

    let current_buf = nvim_oxi::api::get_current_buf();

    let is_term = split_kind.is_term();

    // If current buffer IS terminal OR file buffer (based on the supplied `term` value).
    if (is_term && term_bufs.contains(&current_buf)) || (!is_term && !term_bufs.contains(&current_buf)) {
        ytil_noxi::common::exec_vim_script("only", None)?;
        return Some(());
    }

    // Prioritize focusable floating windows (e.g. fzf-lua picker) over underlying splits.
    if !is_term && let Some(float_win) = ytil_noxi::window::find_focusable_float() {
        ytil_noxi::window::set_current(&float_win)?;
        return Some(());
    }

    // If there is a VISIBLE terminal OR file buffer (based on the supplied `term` value).
    if let Some(win) = nvim_oxi::api::list_wins().find(|win| {
        ytil_noxi::window::get_buffer(win)
            .is_some_and(|buf| (is_term && term_bufs.contains(&buf)) || (!is_term && !term_bufs.contains(&buf)))
    }) {
        ytil_noxi::window::set_current(&win)?;
        return Some(());
    }

    let width = compute_width(width_perc)?;
    let (leftabove, split_new_cmd) = if is_term { ("leftabove ", "term") } else { ("", "enew") };

    // If there is a NON-VISIBLE listed terminal OR file buffer (based on the supplied `term` value).
    if let Some(mru_buf) = mru_bufs
        .iter()
        .find(|b| (is_term && b.is_term()) || (!is_term && matches!(b.kind, BufferKind::Path | BufferKind::NoName)))
    {
        ytil_noxi::common::exec_vim_script(
            &format!("{leftabove}vsplit | vertical resize {width} | buffer {}", mru_buf.id),
            None,
        );
        return Some(());
    }

    // If there is NO terminal buffer OR file buffer at all (based on the supplied `term` value).
    ytil_noxi::common::exec_vim_script(
        &format!("{leftabove}vsplit | vertical resize {width} | {split_new_cmd}"),
        None,
    );

    Some(())
}

fn toggle_alternate_buffer(_: ()) -> Option<()> {
    // Single MRU fetch: "ls t" returns listed buffers in most-recently-used order.
    // The first entry is the current buffer, so skip it and find the first file buffer.
    let mru_bufs = ytil_noxi::mru_buffers::get().unwrap_or_default();

    if let Some(target) = mru_bufs.iter().skip(1).find(|b| matches!(b.kind, BufferKind::Path)) {
        ytil_noxi::buffer::set_current(&Buffer::from(target))?;
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
                && matches!(mru_buffer.kind, BufferKind::Path | BufferKind::NoName)
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
