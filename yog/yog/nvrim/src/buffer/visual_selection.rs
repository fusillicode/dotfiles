use nvim_oxi::Array;
use nvim_oxi::Object;
use nvim_oxi::api::Buffer;
use nvim_oxi::api::opts::GetTextOpts;
use nvim_oxi::conversion::FromObject;
use nvim_oxi::lua::Poppable;
use nvim_oxi::lua::ffi::State;
use serde::Deserialize;
use serde::Deserializer;

use crate::oxi_ext::BufferExt;

/// Extract selected text lines from the current [`Buffer`] using the active Visual range.
///
/// The range endpoints are derived from the current cursor position (`.`) and the Visual
/// start mark (`'v`). This means the function is intended to be invoked while still in
/// Visual mode; if Visual mode has already been exited the mark `'v` may refer to a
/// previous selection and yield stale or unexpected text.
///
/// Mode handling:
/// - Linewise (`V`): returns every full line covered by the selection (columns ignored).
/// - Characterwise (`v`): returns a slice spanning from the start (inclusive) to the end (inclusive) by internally
///   converting the end column to an exclusive bound.
/// - Blockwise (CTRL-V): currently treated like a plain characterwise span; rectangular shape is not preserved.
///
/// On any Neovim API error (fetching marks, lines, or text) a notification is emitted and an
/// empty [`Vec`] is returned. The resulting lines are also passed through [`nvim_oxi::dbg!`]
/// (producing debug output) before being returned.
///
/// # Caveats
///
/// - Relies on the live Visual selection; does not fall back to `'<` / `'>` marks.
/// - Blockwise selections lose their column rectangle shape.
/// - Returned columns for multi-byte UTF-8 characters depend on byte indices exposed by `getpos()`; no grapheme-aware
///   adjustment is performed.
pub fn get(_: ()) -> Vec<nvim_oxi::String> {
    let Ok(cursor_pos) = get_pos(".") else { return vec![] };
    let Ok(visual_pos) = get_pos("v") else { return vec![] };

    let (start_pos, mut end_pos) = cursor_pos.sort(visual_pos);
    let cur_buf = Buffer::current();

    // Handle linewise mode: grab full lines
    if nvim_oxi::api::get_mode().mode == "V" {
        // end.lnum inclusive for lines range
        let Ok(lines) = cur_buf
            .get_lines(start_pos.lnum..=end_pos.lnum, false)
            .inspect_err(|error| {
                crate::oxi_ext::notify_error(&format!("cannot get lines from buffer {cur_buf:#?}, error {error:#?}"));
            })
        else {
            return vec![];
        };
        return lines.collect();
    }

    // Charwise mode:
    // Clamp end.col to line length, then make exclusive by +1 (if not already at end).
    if let Ok(line) = cur_buf.get_line(end_pos.lnum)
        && end_pos.col < line.len()
    {
        end_pos.col = end_pos.col.saturating_add(1); // make exclusive
    }

    // For multi-line charwise selection rely on nvim_buf_get_text with an exclusive end.
    let Ok(lines) = cur_buf
        .get_text(
            start_pos.lnum..end_pos.lnum,
            start_pos.col,
            end_pos.col,
            &GetTextOpts::default(),
        )
        .inspect_err(|error| {
            crate::oxi_ext::notify_error(&format!(
                "cannot get text from buffer {cur_buf:#?} from {start_pos:#?} to {end_pos:#?}, error {error:#?}"
            ));
        })
    else {
        return vec![];
    };
    lines.collect()
}

/// Normalized, 0-based indexed output of Nvim `getpos()`.
///
/// Built from [`RawPos`]. Represents a single position inside a buffer using
/// zero-based (line, column) indices.
#[derive(Debug, PartialEq, Eq)]
pub struct Pos {
    /// 0-based line index.
    pub lnum: usize,
    /// 0-based byte column within the line.
    pub col: usize,
}

impl Pos {
    /// Return `(self, other)` sorted by position, swapping if needed so the first
    /// has the lower (line, column) tuple (columns compared only when on the same line).
    pub const fn sort(self, other: Self) -> (Self, Self) {
        if self.lnum > other.lnum || (self.lnum == other.lnum && self.col > other.col) {
            (other, self)
        } else {
            (self, other)
        }
    }
}

/// Custom [`Deserialize`] from Lua tuple (see [`RawPos`]).
impl<'de> Deserialize<'de> for Pos {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let t = RawPos::deserialize(deserializer)?;
        Ok(Self::from(t))
    }
}

/// Convert [`RawPos`] to [`Pos`] by switching to 0-based indexing from Lua 1-based.
impl From<RawPos> for Pos {
    fn from(raw: RawPos) -> Self {
        fn to_0_based_usize(v: i64) -> usize {
            usize::try_from(v.saturating_sub(1)).unwrap_or_default()
        }

        Self {
            lnum: to_0_based_usize(raw.1),
            col: to_0_based_usize(raw.2),
        }
    }
}

/// Raw `getpos()` tuple: (`bufnum`, `lnum`, `col`, `off`).
#[derive(Debug, Clone, Copy, Deserialize)]
#[expect(dead_code, reason = "Unused fields are kept for completeness")]
struct RawPos(i64, i64, i64, i64);

/// Implementation of [`FromObject`] for [`Pos`].
impl FromObject for Pos {
    fn from_object(obj: Object) -> Result<Self, nvim_oxi::conversion::Error> {
        Self::deserialize(nvim_oxi::serde::Deserializer::new(obj)).map_err(Into::into)
    }
}

/// Implementation of [`Poppable`] for [`Pos`].
impl Poppable for Pos {
    unsafe fn pop(lstate: *mut State) -> Result<Self, nvim_oxi::lua::Error> {
        unsafe {
            let obj = Object::pop(lstate)?;
            Self::from_object(obj).map_err(nvim_oxi::lua::Error::pop_error_from_err::<Self, _>)
        }
    }
}

/// Call Nvim function `getpos()` for the supplied mark identifier and return a normalized [`Pos`].
///
/// On success converts the raw 1-based tuple into 0-based [`Pos`].
/// On failure emits an error notification and returns the underlying error.
///
/// # Parameters
///
/// - `mark`: Mark identifier accepted by `getpos()` (e.g. `"v"` for start of active Visual selection, `"."` for the
///   cursor position).
///
/// # Errors
///
/// Returns an error if the underlying Nvim API call fails or deserialization into [`Pos`] fails.
fn get_pos(mark: &str) -> nvim_oxi::Result<Pos> {
    Ok(
        nvim_oxi::api::call_function::<_, Pos>("getpos", Array::from_iter([mark])).inspect_err(|error| {
            crate::oxi_ext::notify_error(&format!("cannot get pos for {mark}, error {error:#?}"));
        })?,
    )
}
