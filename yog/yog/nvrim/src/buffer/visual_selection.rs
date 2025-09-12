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

/// Return selected text lines from the current [`Buffer`] between the visual start mark and the cursor.
///
/// Produces a [`Vec`] of [`nvim_oxi::String`] (one entry per line) suitable for Lua.
/// If any Nvim API call fails a notification is emitted and an empty [`Vec`] is returned.
/// The end column is adjusted so the last character is included (inclusive selection).
pub fn get(_: ()) -> Vec<nvim_oxi::String> {
    let Ok(cursor_pos) = get_pos(".") else { return vec![] };
    let Ok(visual_pos) = get_pos("v") else { return vec![] };

    let (start_pos, mut end_pos) = cursor_pos.sort(visual_pos);
    let cur_buf = Buffer::current();

    // Handle linewise mode: grab full lines
    if nvim_oxi::api::get_mode().mode == "V" {
        // end.lnum inclusive: need +1 for lines range
        let Ok(lines) = cur_buf
            .get_lines(start_pos.lnum..end_pos.lnum + 1, false)
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
        end_pos.col += 1; // make exclusive
    }

    // For multi-line charwise selection we can rely on nvim_buf_get_text with exclusive end.
    let Ok(iter) = cur_buf.get_text(
        start_pos.lnum..end_pos.lnum,
        start_pos.col,
        end_pos.col,
        &GetTextOpts::default(),
    ) else {
        return vec![];
    };
    nvim_oxi::dbg!(iter.collect())
}

/// Normalized, 0-based indexed output of Nvim `getpos()`.
///
/// Built from [`RawPos`].
#[derive(Debug, PartialEq, Eq)]
pub struct Pos {
    pub lnum: usize,
    pub col: usize,
}

impl Pos {
    /// Return `(self, other)` sorted by position, swapping if needed so the first
    /// has the lower (line, column) tuple.
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

/// Call Nvim function `getpos()` for the supplied mark `pos` and return a normalized [`Pos`].
///
/// On success converts the raw 1-based tuple into 0-based [`Pos`]. On failure emits
/// an error notification and returns the underlying error.
///
/// # Parameters
///
/// - `pos`: Mark character accepted by `getpos()` (e.g. `'v'` for start of visual selection, `'.'` for cursor).
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
