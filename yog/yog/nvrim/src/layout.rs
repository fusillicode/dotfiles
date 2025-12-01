use std::str::FromStr;

use color_eyre::eyre::Context;
use color_eyre::eyre::eyre;
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
        "smart_close_buffer": fn_from!(smart_close_buffer),
        "toggle_alternate_buffer": fn_from!(toggle_alternate_buffer),
    }
}

fn focus_term(_: ()) -> Option<()> {
    let current_buffer = nvim_oxi::api::get_current_buf();

    // If current buffer IS terminal.
    if current_buffer.is_terminal() {
        exec2("only", None)?;
        return Some(());
    }

    // If current buffer IS NOT terminal.
    let mut visible_windows =
        nvim_oxi::api::list_wins().map(|w| (get_window_buffer(&w).and_then(|b| b.get_buf_type()), w));

    let maybe_terminal_window = visible_windows.find(|(bt, _)| bt.as_ref().is_some_and(|b| b == "terminal"));

    // If there is a VISIBLE terminal buffer.
    if let Some((_, win)) = maybe_terminal_window {
        set_current_window(&win)?;
        exec2("startinsert", None)?;
        return Some(());
    }

    let width = compute_width(TERM_WIDTH_PERC)?;

    // If there is NO VISIBLE terminal buffer.
    if let Some(terminal_buffer) = nvim_oxi::api::list_bufs().find(BufferExt::is_terminal) {
        exec2(&format!("leftabove vsplit | vertical resize {width}"), None);
        set_current_buffer(&terminal_buffer)?;
        exec2("startinsert", None)?;
        return Some(());
    }

    // If there is NO terminal buffer at all.
    exec2(&format!("leftabove vsplit | vertical resize {width} | term"), None);
    exec2("startinsert", None)?;

    Some(())
}

fn focus_buffer(_: ()) -> Option<()> {
    let current_buffer = nvim_oxi::api::get_current_buf();

    // If current buffer IS NOT terminal.
    if !current_buffer.is_terminal() {
        exec2("only", None)?;
        return Some(());
    }

    // If current buffer IS terminal.
    let mut visible_windows =
        nvim_oxi::api::list_wins().map(|w| (get_window_buffer(&w).and_then(|b| b.get_buf_type()), w));

    let maybe_buffer_window = visible_windows.find(|(bt, _)| bt.as_ref().is_some_and(|b| b.is_empty()));

    // If there is a visible file buffer.
    if let Some((_, win)) = maybe_buffer_window {
        set_current_window(&win)?;
        return Some(());
    }

    // If there is NO visible file buffer.
    let width = compute_width(FILE_BUF_WIDTH_PERC)?;

    // Using exec2 because nvim_oxi::api::open_win fails with split left.
    exec2(&format!("vsplit | vertical resize {width}"), None)?;

    let buffer = if let Some(mru_buffer) = get_mru_buffers()?
        .iter()
        .find(|b| matches!(b.kind, BufferKind::Path | BufferKind::NoName))
    {
        Buffer::from(mru_buffer)
    } else {
        create_buffer()?
    };

    set_current_buffer(&buffer)?;

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

#[allow(dead_code)]
fn get_jumplist() -> Option<Vec<JumpEntry>> {
    Some(
        nvim_oxi::api::call_function::<_, JumpList>("getjumplist", Array::new())
            .inspect_err(|err| ytil_nvim_oxi::notify::error(format!("error getting jumplist | err={err:?}")))
            .ok()?
            .0,
    )
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
        set_current_buffer(&alt_buf)?;
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
            set_current_buffer(&buf)?;
            return Some(());
        }
    }

    Some(())
}

fn smart_close_buffer(force_close: Option<bool>) -> Option<()> {
    let mru_buffers = get_mru_buffers()?;

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
                create_buffer()?
            };

            set_current_buffer(&new_current_buffer)?;
        }
    };

    exec2(&format!("bd{force} {}", current_buffer.id), Default::default())?;

    Some(())
}

#[derive(Debug)]
#[allow(dead_code)]
struct MruBuffer {
    pub id: i32,
    pub is_unlisted: bool,
    pub name: String,
    pub kind: BufferKind,
}

impl From<&MruBuffer> for Buffer {
    fn from(value: &MruBuffer) -> Self {
        Buffer::from(value.id)
    }
}

#[derive(Debug)]
enum BufferKind {
    Term,
    GrugFar,
    Path,
    NoName,
}

impl<T: AsRef<str>> From<T> for BufferKind {
    fn from(value: T) -> Self {
        let str = value.as_ref();
        if str.starts_with("term://") {
            Self::Term
        } else if str.starts_with("Grug FAR") {
            Self::GrugFar
        } else if str.starts_with("[No Name]") {
            Self::NoName
        } else {
            Self::Path
        }
    }
}

impl FromStr for MruBuffer {
    type Err = color_eyre::eyre::Error;

    fn from_str(mru_buffer_line: &str) -> Result<Self, Self::Err> {
        let mru_buffer_line = mru_buffer_line.trim();

        let is_unlisted_idx = mru_buffer_line
            .char_indices()
            .find_map(|(idx, c)| if !c.is_numeric() { Some(idx) } else { None })
            .ok_or_else(|| eyre!("error finding buffer id end | mru_buffer_line={mru_buffer_line:?}"))?;

        let id: i32 = {
            let id = mru_buffer_line
                .get(..is_unlisted_idx)
                .ok_or_else(|| eyre!("error extracting buffer id | mru_buffer_line={mru_buffer_line:?}"))?;
            id.parse()
                .wrap_err_with(|| format!("error parsing buffer id | id={id:?} mru_buffer_line={mru_buffer_line:?}"))?
        };

        let is_unlisted = mru_buffer_line.get(is_unlisted_idx..=is_unlisted_idx).ok_or_else(|| {
            eyre!("error extracting is_unlisted by idx | idx={is_unlisted_idx} mru_buffer_line={mru_buffer_line:?}")
        })? == "u";

        // Skip entirely the other flags and the first '"' char.
        let name_idx = is_unlisted_idx.saturating_add(7);

        let rest = mru_buffer_line.get(name_idx..).ok_or_else(|| {
            eyre!("error extracting name part by idx | idx={name_idx} mru_buffer_line={mru_buffer_line:?}")
        })?;

        let (name, _) = rest
            .split_once('"')
            .ok_or_else(|| eyre!("error extracting name | rest={rest:?} mru_buffer_line={mru_buffer_line:?}"))?;

        Ok(Self {
            id,
            is_unlisted,
            name: name.to_string(),
            kind: BufferKind::from(name),
        })
    }
}

fn get_mru_buffers() -> Option<Vec<MruBuffer>> {
    let Ok(mru_buffers_output) = nvim_oxi::api::call_function::<_, String>("execute", ("ls t",))
        .inspect_err(|err| ytil_nvim_oxi::notify::error(format!("error getting mru buffers | err={err:?}")))
    else {
        return None;
    };

    parse_mru_buffers_output(&mru_buffers_output)
        .inspect_err(|err| {
            ytil_nvim_oxi::notify::error(format!(
                "error parsing mru buffers output | mru_buffers_output={mru_buffers_output:?} err={err:?}"
            ))
        })
        .ok()
}

fn parse_mru_buffers_output(mru_buffers_output: &str) -> color_eyre::Result<Vec<MruBuffer>> {
    if mru_buffers_output.is_empty() {
        return Ok(vec![]);
    }
    let mut out = vec![];
    for mru_buffer_line in mru_buffers_output.lines() {
        if mru_buffer_line.is_empty() {
            continue;
        }
        out.push(MruBuffer::from_str(mru_buffer_line)?)
    }
    Ok(out)
}

fn set_current_buffer(buf: &Buffer) -> Option<()> {
    nvim_oxi::api::set_current_buf(buf)
        .inspect_err(|err| {
            ytil_nvim_oxi::notify::error(format!("error setting current buffer | buffer={buf:?} err={err:?}"))
        })
        .ok()?;
    Some(())
}

fn set_current_window(window: &Window) -> Option<()> {
    nvim_oxi::api::set_current_win(window)
        .inspect_err(|err| {
            ytil_nvim_oxi::notify::error(format!("error setting current window | window={window:?}, err={err:?}"))
        })
        .ok()?;
    Some(())
}

fn get_window_buffer(win: &Window) -> Option<Buffer> {
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

#[allow(dead_code)]
fn get_alt_buffer_or_new() -> Option<Buffer> {
    let alt_buf_id = nvim_oxi::api::call_function::<_, i32>("bufnr", ("#",))
        .inspect(|err| {
            ytil_nvim_oxi::notify::error(format!("error getting alternate buffer | err={err:?}"));
        })
        .ok()?;

    if alt_buf_id < 0 {
        return create_buffer();
    }
    Some(Buffer::from(alt_buf_id))
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

fn create_buffer() -> Option<Buffer> {
    nvim_oxi::api::create_buf(true, false)
        .inspect_err(|err| ytil_nvim_oxi::notify::error(format!("error creating buffer | err={err:?}")))
        .ok()
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
