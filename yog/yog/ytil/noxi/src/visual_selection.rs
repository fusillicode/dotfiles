//! Visual selection extraction helpers.

use std::ops::Range;

use nvim_oxi::Array;
use nvim_oxi::Dictionary;
use nvim_oxi::Object;
use nvim_oxi::api::Buffer;
use nvim_oxi::api::SuperIterator;
use nvim_oxi::api::opts::GetTextOpts;
use nvim_oxi::conversion::FromObject;
use nvim_oxi::lua::Poppable;
use nvim_oxi::lua::ffi::State;
use rootcause::prelude::ResultExt as _;
use rootcause::report;
use serde::Deserialize;
use serde::Deserializer;

use crate::buffer::BufferExt;
use crate::dict;

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
/// On any Nvim API error (fetching marks, lines, or text) a notification is emitted and an
/// empty [`Vec`] is returned.
///
/// # Caveats
/// - Relies on the live Visual selection; does not fall back to `'<` / `'>` marks.
/// - Blockwise selections lose their column rectangle shape.
/// - Returned columns for multi-byte UTF-8 characters depend on byte indices exposed by `getpos()`; no grapheme-aware
///   adjustment is performed.
pub fn get_lines(_: ()) -> Vec<String> {
    get(()).map_or_else(Vec::new, |f| f.lines)
}

/// Extract the last Visual selection using persisted `'<` / `'>` marks.
///
/// This is meant for integrations that leave Visual mode before acting on the selected text.
pub fn get_marked(_: ()) -> Option<Dictionary> {
    let selection = get_from_visual_marks(())?;

    Some(selection_to_dict(&selection))
}

/// Extract the persisted Visual selection if it matches the given Ex range; otherwise return the full line range.
///
/// The returned dictionary is intentionally format-agnostic: it contains selected lines, 0-based buffer coordinates,
/// and the command prefix needed by Lua integrations.
pub fn get_for_ex_range((line1, line2): (usize, usize)) -> Option<Dictionary> {
    get_from_visual_marks(())
        .filter(|selection| selection.matches_ex_range(line1, line2))
        .or_else(|| get_line_range_selection(line1, line2))
        .map(|selection| selection_to_dict(&selection))
}

/// Return the command-line range prefix for a persisted Visual selection.
pub fn get_visual_range_command_prefix(_: ()) -> Option<String> {
    let bounds = SelectionBounds::from_visual_marks()
        .inspect_err(|err| {
            crate::notify::error(format!("error creating visual selection bounds | error={err:#?}"));
        })
        .ok()?;

    Some(visual_range_command_prefix(&bounds))
}

/// Return an owned [`Selection`] for the active Visual range.
///
/// On any Nvim API error (fetching marks, lines, or text) a notification is emitted and [`None`] is returned.
///
/// # Errors
/// - Return [`None`] if retrieving either mark fails.
/// - Return [`None`] if the two marks reference different buffers.
/// - Return [`None`] if getting lines or text fails.
pub fn get(_: ()) -> Option<Selection> {
    let mut bounds = SelectionBounds::new()
        .inspect_err(|err| {
            crate::notify::error(format!("error creating selection bounds | error={err:#?}"));
        })
        .ok()?;

    get_selection(&mut bounds, nvim_oxi::api::get_mode().mode == "V")
}

/// Return an owned [`Selection`] for the persisted Visual marks.
///
/// On any Nvim API error (fetching marks, lines, or text) a notification is emitted and [`None`] is returned.
pub fn get_from_visual_marks(_: ()) -> Option<Selection> {
    let mut bounds = SelectionBounds::from_visual_marks()
        .inspect_err(|err| {
            crate::notify::error(format!("error creating visual selection bounds | error={err:#?}"));
        })
        .ok()?;

    get_selection(&mut bounds, last_visual_mode().as_deref() == Some("V"))
}

fn get_selection(bounds: &mut SelectionBounds, is_linewise: bool) -> Option<Selection> {
    let current_buffer = Buffer::from(bounds.buf_id());

    // Handle linewise mode: grab full lines
    if is_linewise {
        let end_lnum = bounds.end().lnum;
        let last_line = current_buffer
            .get_line(end_lnum)
            .inspect_err(|err| {
                crate::notify::error(format!(
                    "error getting selection last line | end_lnum={end_lnum} buffer={current_buffer:#?} error={err:#?}",
                ));
            })
            .ok()?;
        // Adjust bounds to start at column 0 and end at the last line's length
        bounds.start.col = 0;
        bounds.end.col = last_line.len();
        // end.lnum inclusive for lines range
        let lines = current_buffer
            .get_lines(bounds.start().lnum..=bounds.end().lnum, false)
            .inspect_err(|err| {
                crate::notify::error(format!(
                    "error getting lines | buffer={current_buffer:#?} error={err:#?}"
                ));
            })
            .ok()?;
        return Some(Selection::new(bounds.clone(), lines));
    }

    // Charwise mode:
    // Clamp end.col to line length, then make exclusive by +1 (if not already at end).
    if let Ok(line) = current_buffer.get_line(bounds.end().lnum)
        && bounds.end().col < line.len()
    {
        bounds.incr_end_col(); // make exclusive
    }

    // For multi-line charwise selection rely on `nvim_buf_get_text` with an exclusive end.
    let lines = current_buffer
        .get_text(
            bounds.line_range(),
            bounds.start().col,
            bounds.end().col,
            &GetTextOpts::default(),
        )
        .inspect_err(|err| {
            crate::notify::error(format!(
                "error getting text | buffer={current_buffer:#?} bounds={bounds:#?} error={err:#?}"
            ));
        })
        .ok()?;

    Some(Selection::new(bounds.clone(), lines))
}

fn last_visual_mode() -> Option<String> {
    nvim_oxi::api::call_function::<_, String>("visualmode", Array::new())
        .inspect_err(|err| {
            crate::notify::error(format!("error getting last visual mode | error={err:#?}"));
        })
        .ok()
}

fn selection_to_dict(selection: &Selection) -> Dictionary {
    dict! {
        "lines": selection.lines().iter().map(String::as_str).collect::<Array>(),
        "start": bound_to_array(selection.start()),
        "end": bound_to_array(selection.end()),
        "command_prefix": visual_range_command_prefix(&selection.bounds),
    }
}

fn bound_to_array(bound: &Bound) -> Array {
    Array::from_iter([usize_to_i64(bound.lnum), usize_to_i64(bound.col)])
}

fn usize_to_i64(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn visual_range_command_prefix(_bounds: &SelectionBounds) -> String {
    "'<,'>".to_owned()
}

fn get_line_range_selection(line1: usize, line2: usize) -> Option<Selection> {
    if line2 < line1 {
        return None;
    }
    let start_lnum = line1.checked_sub(1)?;
    let end_lnum = line2.checked_sub(1)?;
    let current_buffer = Buffer::current();
    let last_line = current_buffer
        .get_line(end_lnum)
        .inspect_err(|err| {
            crate::notify::error(format!(
                "error getting range last line | end_lnum={end_lnum} buffer={current_buffer:#?} error={err:#?}",
            ));
        })
        .ok()?;
    let selected_lines = current_buffer
        .get_lines(start_lnum..=end_lnum, true)
        .inspect_err(|err| {
            crate::notify::error(format!(
                "error getting range lines | buffer={current_buffer:#?} error={err:#?}"
            ));
        })
        .ok()?;
    let bounds = SelectionBounds {
        buf_id: current_buffer.handle(),
        start: Bound {
            lnum: start_lnum,
            col: 0,
        },
        end: Bound {
            lnum: end_lnum,
            col: last_line.len(),
        },
    };

    Some(Selection::new(bounds, selected_lines))
}

/// Owned selection content plus bounds.
#[derive(Debug)]
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
#[derive(Clone, Debug)]
pub struct SelectionBounds {
    #[cfg(feature = "testing")]
    pub buf_id: i32,
    #[cfg(feature = "testing")]
    pub start: Bound,
    #[cfg(feature = "testing")]
    pub end: Bound,
    #[cfg(not(feature = "testing"))]
    buf_id: i32,
    #[cfg(not(feature = "testing"))]
    start: Bound,
    #[cfg(not(feature = "testing"))]
    end: Bound,
}

impl SelectionBounds {
    /// Builds selection bounds from the current cursor (`.`) and visual start (`v`) marks.
    ///
    /// Retrieves positions using Nvim's `getpos()` function and normalizes them to 0-based indices.
    /// The start and end are sorted to ensure start is before end.
    ///
    /// # Errors
    /// - Fails if retrieving either mark fails.
    /// - Fails if the two marks reference different buffers.
    pub fn new() -> rootcause::Result<Self> {
        let cursor_pos = get_pos(".")?;
        let visual_pos = get_pos("v")?;

        Self::from_positions(cursor_pos, visual_pos)
    }

    /// Builds selection bounds from the persisted Visual selection marks.
    ///
    /// # Errors
    /// - Fails if retrieving either mark fails.
    /// - Fails if the two marks reference different buffers.
    pub fn from_visual_marks() -> rootcause::Result<Self> {
        let start_pos = get_pos("'<")?;
        let end_pos = get_pos("'>")?;

        Self::from_positions(start_pos, end_pos)
    }

    fn from_positions(first: Pos, second: Pos) -> rootcause::Result<Self> {
        let (start, end) = first.sort(second);

        if start.buf_id != end.buf_id {
            Err(report!("mismatched buffer ids")).attach_with(|| format!("start={start:#?} end={end:#?}"))?;
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
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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

    fn matches_ex_range(&self, line1: usize, line2: usize) -> bool {
        self.start().lnum.checked_add(1) == Some(line1) && self.end().lnum.checked_add(1) == Some(line2)
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
        // SAFETY: The caller (nvim-oxi framework) guarantees that:
        // 1. `lstate` is a valid pointer to an initialized Lua state
        // 2. The Lua stack has at least one value to pop
        unsafe {
            let obj = Object::pop(lstate)?;
            Self::from_object(obj).map_err(nvim_oxi::lua::Error::pop_error_from_err::<Self, _>)
        }
    }
}

/// Calls Nvim's `getpos()` function for the supplied mark identifier and returns a normalized [`Pos`].
///
/// On success, converts the raw 1-based tuple into a 0-based [`Pos`].
/// On failure, emits an error notification via [`crate::notify::error`] and wraps the error with
/// additional context using [`rootcause`].
///
/// # Errors
/// - Calling `getpos()` fails.
/// - Deserializing the returned tuple into [`Pos`] fails.
fn get_pos(mark: &str) -> rootcause::Result<Pos> {
    Ok(
        nvim_oxi::api::call_function::<_, Pos>("getpos", Array::from_iter([mark]))
            .inspect_err(|err| {
                crate::notify::error(format!("error getting pos | mark={mark:?} error={err:#?}"));
            })
            .context("error getting position")
            .attach_with(|| format!("mark={mark:?}"))?,
    )
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::self_has_lower_line(pos(0, 5), pos(1, 0), pos(0, 5), pos(1, 0))]
    #[case::self_has_higher_line(pos(2, 0), pos(1, 5), pos(1, 5), pos(2, 0))]
    #[case::same_line_self_lower_col(pos(1, 0), pos(1, 5), pos(1, 0), pos(1, 5))]
    #[case::same_line_self_higher_col(pos(1, 10), pos(1, 5), pos(1, 5), pos(1, 10))]
    #[case::positions_identical(pos(1, 5), pos(1, 5), pos(1, 5), pos(1, 5))]
    fn test_pos_sort_returns_expected_order(
        #[case] self_pos: Pos,
        #[case] other_pos: Pos,
        #[case] expected_first: Pos,
        #[case] expected_second: Pos,
    ) {
        let (first, second) = self_pos.sort(other_pos);
        pretty_assertions::assert_eq!(first, expected_first);
        pretty_assertions::assert_eq!(second, expected_second);
    }

    fn pos(lnum: usize, col: usize) -> Pos {
        Pos { buf_id: 1, lnum, col }
    }

    #[test]
    fn test_selection_bounds_from_positions_normalizes_reversed_coordinates() {
        let result = SelectionBounds::from_positions(pos(4, 10), pos(2, 3));

        assert2::assert!(let Ok(bounds) = result);
        pretty_assertions::assert_eq!(*bounds.start(), Bound { lnum: 2, col: 3 });
        pretty_assertions::assert_eq!(*bounds.end(), Bound { lnum: 4, col: 10 });
    }

    #[test]
    fn test_selection_lines_returns_raw_selected_lines() {
        let result = SelectionBounds::from_positions(pos(1, 2), pos(2, 8));

        assert2::assert!(let Ok(bounds) = result);
        let selection = Selection::new(
            bounds,
            vec![nvim_oxi::String::from("{\"b\":2}"), nvim_oxi::String::from("x")].into_iter(),
        );

        pretty_assertions::assert_eq!(selection.lines(), &["{\"b\":2}".to_string(), "x".to_string()]);
    }

    #[test]
    fn test_visual_range_command_prefix_returns_visual_range() {
        let result = SelectionBounds::from_positions(pos(1, 0), pos(3, 4));

        assert2::assert!(let Ok(bounds) = result);
        pretty_assertions::assert_eq!(visual_range_command_prefix(&bounds), "'<,'>");
    }
}
