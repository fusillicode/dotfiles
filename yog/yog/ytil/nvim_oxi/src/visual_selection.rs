//! Visual selection extraction helpers.
//!
//! Provides functions to capture current Visual mode text (line‑wise or character‑wise) into owned
//! structures (`Selection`, `SelectionBounds`) with normalized 0‑based indices.

use std::ops::Range;

use color_eyre::eyre::bail;
use color_eyre::eyre::eyre;
use nvim_oxi::Array;
use nvim_oxi::Object;
use nvim_oxi::api::Buffer;
use nvim_oxi::api::SuperIterator;
use nvim_oxi::api::opts::GetTextOpts;
use nvim_oxi::conversion::FromObject;
use nvim_oxi::lua::Poppable;
use nvim_oxi::lua::ffi::State;
use serde::Deserialize;
use serde::Deserializer;

use crate::buffer::BufferExt;

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
/// - Relies on the live Visual selection; does not fall back to `'<` / `'>` marks.
/// - Blockwise selections lose their column rectangle shape.
/// - Returned columns for multi-byte UTF-8 characters depend on byte indices exposed by `getpos()`; no grapheme-aware
///   adjustment is performed.
pub fn get_lines(_: ()) -> Vec<String> {
    get(()).map_or_else(Vec::new, |f| f.lines)
}

/// Return an owned [`Selection`] for the active Visual range.
///
/// On any Neovim API error (fetching marks, lines, or text) a notification is emitted and [`None`] is returned.
///
/// # Returns
/// Returns `Some(Selection)` if the visual selection can be extracted successfully, [`None`] otherwise.
///
/// # Errors
/// - Return [`None`] if retrieving either mark fails.
/// - Return [`None`] if the two marks reference different buffers.
/// - Return [`None`] if getting lines or text fails.
pub fn get(_: ()) -> Option<Selection> {
    let Ok(mut bounds) = SelectionBounds::new().inspect_err(|error| {
        crate::api::notify_error(&format!("cannot create selection bounds | error={error:#?}"));
    }) else {
        return None;
    };

    let cur_buf = Buffer::from(bounds.buf_id());

    // Handle linewise mode: grab full lines
    if nvim_oxi::api::get_mode().mode == "V" {
        // end.lnum inclusive for lines range
        let Ok(lines) = cur_buf
            .get_lines(bounds.start().lnum..=bounds.end().lnum, false)
            .inspect_err(|error| {
                crate::api::notify_error(&format!("cannot get lines | buffer={cur_buf:#?} error={error:#?}"));
            })
        else {
            return None;
        };
        return Some(Selection::new(bounds, lines));
    }

    // Charwise mode:
    // Clamp end.col to line length, then make exclusive by +1 (if not already at end).
    if let Ok(line) = cur_buf.get_line(bounds.end().lnum)
        && bounds.end().col < line.len()
    {
        bounds.incr_end_col(); // make exclusive
    }

    // For multi-line charwise selection rely on `nvim_buf_get_text` with an exclusive end.
    let Ok(lines) = cur_buf
        .get_text(
            bounds.line_range(),
            bounds.start().col,
            bounds.end().col,
            &GetTextOpts::default(),
        )
        .inspect_err(|error| {
            crate::api::notify_error(&format!(
                "cannot get text | buffer={cur_buf:#?} bounds={bounds:#?} error={error:#?}"
            ));
        })
    else {
        return None;
    };

    Some(Selection::new(bounds, lines))
}

/// Owned selection content plus bounds.
pub struct Selection {
    bounds: SelectionBounds,
    lines: Vec<String>,
}

impl Selection {
    /// Create a new [`Selection`] from bounds and raw line objects.
    pub fn new(bounds: SelectionBounds, lines: impl SuperIterator<nvim_oxi::String>) -> Self {
        Self {
            bounds,
            lines: lines.into_iter().map(|line| line.to_string()).collect(),
        }
    }
}

/// Start / end bounds plus owning buffer id for a Visual selection.
#[derive(Clone, Copy, Debug)]
pub struct SelectionBounds {
    buf_id: i32,
    start: Bound,
    end: Bound,
}

impl SelectionBounds {
    /// Builds selection bounds from the current cursor (`.`) and visual start (`v`) marks.
    ///
    /// Retrieves positions using Neovim's `getpos()` function and normalizes them to 0-based indices.
    /// The start and end are sorted to ensure start is before end.
    ///
    /// # Returns
    /// Returns a new `SelectionBounds` instance with normalized start and end positions.
    ///
    /// # Errors
    /// - Fails if retrieving either mark fails.
    /// - Fails if the two marks reference different buffers.
    pub fn new() -> color_eyre::Result<Self> {
        let cursor_pos = get_pos(".")?;
        let visual_pos = get_pos("v")?;

        let (start, end) = cursor_pos.sort(visual_pos);

        if start.buf_id != end.buf_id {
            bail!("mismatched buffer ids | start={start:#?} end={end:#?}")
        }

        Ok(Self {
            buf_id: start.buf_id,
            start: Bound::from(start),
            end: Bound::from(end),
        })
    }

    /// Range of starting (inclusive) to ending (exclusive) line indices.
    pub const fn line_range(&self) -> Range<usize> {
        self.start.lnum..self.end.lnum
    }

    /// Owning buffer id.
    pub const fn buf_id(&self) -> i32 {
        self.buf_id
    }

    /// Start bound.
    pub const fn start(&self) -> &Bound {
        &self.start
    }

    /// End bound (exclusive line, exclusive column for charwise mode after adjustment).
    pub const fn end(&self) -> &Bound {
        &self.end
    }

    /// Increment end column (making it exclusive for charwise selections).
    const fn incr_end_col(&mut self) {
        self.end.col = self.end.col.saturating_add(1);
    }
}

/// Single position (line, column) inside a buffer.
#[derive(Clone, Copy, Debug)]
pub struct Bound {
    /// 0-based line number.
    pub lnum: usize,
    /// 0-based byte column.
    pub col: usize,
}

impl From<Pos> for Bound {
    fn from(value: Pos) -> Self {
        Self {
            lnum: value.lnum,
            col: value.col,
        }
    }
}

impl Selection {
    /// Buffer id containing the selection.
    pub const fn buf_id(&self) -> i32 {
        self.bounds.buf_id()
    }

    /// Start bound of the selection.
    pub const fn start(&self) -> &Bound {
        self.bounds.start()
    }

    /// End bound of the selection.
    pub const fn end(&self) -> &Bound {
        self.bounds.end()
    }

    /// Collected selected lines.
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    /// Range of starting (inclusive) to ending (exclusive) line indices.
    pub const fn line_range(&self) -> Range<usize> {
        self.bounds.line_range()
    }
}

/// Normalized, 0-based indexed output of Nvim `getpos()`.
///
/// Built from internal `RawPos` (private). Represents a single position inside a buffer using
/// zero-based (line, column) indices.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Pos {
    buf_id: i32,
    /// 0-based line index.
    lnum: usize,
    /// 0-based byte column within the line.
    col: usize,
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

/// Custom [`Deserialize`] from Lua tuple produced by `getpos()` (via internal `RawPos`).
impl<'de> Deserialize<'de> for Pos {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let t = RawPos::deserialize(deserializer)?;
        Ok(Self::from(t))
    }
}

/// Convert internal `RawPos` to [`Pos`] by switching to 0-based indexing from Lua 1-based.
impl From<RawPos> for Pos {
    fn from(raw: RawPos) -> Self {
        fn to_0_based_usize(v: i64) -> usize {
            usize::try_from(v.saturating_sub(1)).unwrap_or_default()
        }

        Self {
            buf_id: raw.0,
            lnum: to_0_based_usize(raw.1),
            col: to_0_based_usize(raw.2),
        }
    }
}

/// Raw `getpos()` tuple: (`bufnum`, `lnum`, `col`, `off`).
#[derive(Clone, Copy, Debug, Deserialize)]
#[expect(dead_code, reason = "Unused fields are kept for completeness")]
struct RawPos(i32, i64, i64, i64);

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

/// Calls Neovim's `getpos()` function for the supplied mark identifier and returns a normalized [`Pos`].
///
/// On success, converts the raw 1-based tuple into a 0-based [`Pos`].
/// On failure, emits an error notification via [`crate::api::notify_error`] and wraps the error with
/// additional context using [`color_eyre::eyre`].
///
/// # Arguments
/// - `mark` Mark identifier accepted by `getpos()` (e.g. `"v"` for start of active Visual selection, `"."` for the
///   cursor position).
///
/// # Errors
/// - Calling `getpos()` fails.
/// - Deserializing the returned tuple into [`Pos`] fails.
fn get_pos(mark: &str) -> color_eyre::Result<Pos> {
    nvim_oxi::api::call_function::<_, Pos>("getpos", Array::from_iter([mark]))
        .inspect_err(|error| {
            crate::api::notify_error(&format!("cannot get pos | mark={mark} error={error:#?}"));
        })
        .map_err(|error| eyre!("get_pos failed | mark={mark:?} error={error:#?}"))
}
