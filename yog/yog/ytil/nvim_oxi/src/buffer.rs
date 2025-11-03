//! Buffer extension utilities (line access, cursor‑based insertion, cursor position model).
//!
//! Supplies [`BufferExt`] trait plus [`CursorPosition`] struct preserving raw Neovim coordinates for
//! consistent conversions at call sites.

use color_eyre::eyre::eyre;
use nvim_oxi::api::Buffer;
use nvim_oxi::api::Window;
use nvim_oxi::api::opts::GetTextOpts;

/// Extension trait for [`Buffer`] to provide extra functionalities.
///
/// Provides focused helpers for line fetching and text insertion at the current
/// cursor position while surfacing Neovim errors via `notify_error`.
pub trait BufferExt {
    /// Fetch a single line from a [`Buffer`] by 0-based index.
    ///
    /// Returns a [`color_eyre::Result`] with the line as [`nvim_oxi::String`].
    /// Errors if the line does not exist at `idx`.
    ///
    /// # Arguments
    /// - `idx` 0-based line index inside the buffer.
    ///
    /// # Errors
    /// - Fetching the line via `nvim_buf_get_lines` fails.
    /// - The requested index is out of range (no line returned).
    fn get_line(&self, idx: usize) -> color_eyre::Result<nvim_oxi::String>;

    /// Inserts `text` at the current cursor position in the active buffer.
    ///
    /// Obtains the current [`CursorPosition`], converts the 1-based row to 0-based
    /// for Neovim's `set_text` call, and inserts `text` without replacing existing
    /// content (`start_col` == `end_col`). Errors are reported via `notify_error`.
    /// Silently returns if cursor position cannot be fetched.
    ///
    /// # Arguments
    /// - `text` UTF-8 slice inserted at the cursor byte column.
    fn set_text_at_cursor_pos(&mut self, text: &str);

    /// Get text from a [`nvim_oxi::api::Buffer`].
    ///
    /// Retrieves lines from the specified start position to end position (inclusive), converting
    /// each line to a [`String`].
    ///
    /// # Arguments
    /// - `start` (lnum, col) 0-based starting line and column (column is byte offset).
    /// - `end` (end_lnum, end_col) 0-based ending line and column (inclusive; column is byte offset).
    /// - `opts` Reference to [`GetTextOpts`] for additional options.
    ///
    /// # Returns
    /// - `Ok(Vec<String>)` with the extracted lines.
    ///
    /// # Errors
    /// - Propagates [`nvim_oxi::api::Error`] from the underlying `nvim_buf_get_text` call.
    fn get_text_between(
        &self,
        start: (usize, usize),
        end: (usize, usize),
        opts: &GetTextOpts,
    ) -> Result<Vec<String>, nvim_oxi::api::Error>;
}

impl BufferExt for Buffer {
    /// Get line.
    fn get_line(&self, idx: usize) -> color_eyre::Result<nvim_oxi::String> {
        self.get_lines(idx..=idx, true)?
            .next()
            .ok_or_else(|| eyre!("buffer line missing | idx={idx} buffer={self:#?}"))
    }

    /// Insert text at cursor.
    fn set_text_at_cursor_pos(&mut self, text: &str) {
        let Some(cur_pos) = CursorPosition::get_current() else {
            return;
        };

        let row = cur_pos.row.saturating_sub(1);
        let line_range = row..row;
        let start_col = cur_pos.col;
        let end_col = cur_pos.col;
        let text = vec![text];

        if let Err(error) = self.set_text(line_range.clone(), start_col, end_col, text.clone()) {
            crate::api::notify_error(format!(
                "cannot set text in buffer | text={text:?} buffer={self:?} line_range={line_range:?} start_col={start_col:?} end_col={end_col:?} error={error:?}",
            ));
        }
    }

    fn get_text_between(
        &self,
        (start_lnum, start_col): (usize, usize),
        (end_lnum, end_col): (usize, usize),
        opts: &GetTextOpts,
    ) -> Result<Vec<String>, nvim_oxi::api::Error> {
        Ok(self
            .get_text(start_lnum..end_lnum, start_col, end_col, opts)?
            .map(|line| line.to_string())
            .collect::<Vec<_>>())
    }
}

/// Represents the current cursor coordinates in the active [`Window`].
///
/// Row is 1-based (Neovim convention) and column is 0-based (byte index inside
/// the line per Neovim API). These are kept verbatim to avoid off-by-one bugs.
/// Call sites converting to Rust slice indices subtract 1 from `row` as needed.
///
/// # Assumptions
/// - Constructed through [`CursorPosition::get_current`]; manual construction should respect coordinate conventions.
///
/// # Rationale
/// Preserving raw Neovim values centralizes conversion logic at usage points
/// (e.g. buffer line indexing) instead of embedding heuristics here.
#[derive(Debug)]
pub struct CursorPosition {
    pub row: usize,
    pub col: usize,
}

impl CursorPosition {
    /// Obtains the current cursor position from the active [`Window`].
    ///
    /// Queries Neovim for the (row, col) of the active window cursor and returns a
    /// [`CursorPosition`] reflecting those raw coordinates.
    ///
    /// # Returns
    /// - `Some(CursorPosition)` when the cursor location is successfully fetched.
    /// - `None` if Neovim fails to provide the cursor position (an error is already reported via `notify_error`).
    ///
    /// # Assumptions
    /// - Row is 1-based (Neovim convention); column is 0-based. Callers needing 0-based row for Rust indexing must
    ///   subtract 1 explicitly.
    /// - The active window is the intended source of truth for cursor location.
    ///
    /// # Rationale
    /// Returning `Option` (instead of `Result`) simplifies common call sites that
    /// treat absence as a soft failure (e.g. skipping an insertion). Detailed
    /// error context is still surfaced to the user through `notify_error`.
    pub fn get_current() -> Option<Self> {
        let cur_win = Window::current();
        let Ok((row, col)) = cur_win.get_cursor().inspect_err(|error| {
            crate::api::notify_error(format!("cannot get cursor | window={cur_win:?} error={error:?}"));
        }) else {
            return None;
        };
        Some(Self { row, col })
    }

    /// Returns 1-based column index for rendering purposes.
    ///
    /// Converts the raw 0-based Neovim column stored in [`CursorPosition::col`] into a
    /// human-friendly 1-based column suitable for statusline / UI output.
    ///
    /// # Returns
    /// - The 1-based column index (`self.col + 1`).
    ///
    /// # Assumptions
    /// - [`CursorPosition::col`] is the unmodified 0-based byte offset provided by Neovim.
    ///
    /// # Rationale
    /// Neovim exposes a 0-based column while rows are 1-based. Normalizing to 1-based for
    /// display avoids mixed-base confusion in user-facing components (e.g. status line) and
    /// clarifies intent at call sites.
    ///
    /// # Performance
    /// Constant time. Uses `saturating_add` defensively (overflow is unrealistic given line length).
    pub const fn adjusted_col(&self) -> usize {
        self.col.saturating_add(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_position_adjusted_col_when_zero_returns_one() {
        let pos = CursorPosition { row: 1, col: 0 };
        pretty_assertions::assert_eq!(pos.adjusted_col(), 1);
    }

    #[test]
    fn cursor_position_adjusted_col_when_non_zero_increments_by_one() {
        let pos = CursorPosition { row: 10, col: 7 };
        pretty_assertions::assert_eq!(pos.adjusted_col(), 8);
    }
}
