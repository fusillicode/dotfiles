use color_eyre::eyre::eyre;
use nvim_oxi::api::Buffer;
use nvim_oxi::api::Window;

/// Extension trait for [`Buffer`] to provide extra functionalities.
pub trait BufferExt {
    /// Fetch a single line from a [`Buffer`] by 0-based index.
    ///
    /// Returns a [`color_eyre::Result`] with the line as [`nvim_oxi::String`].
    /// Errors if the line does not exist at `idx`.
    ///
    /// # Errors
    /// - Fetching the line via `nvim_buf_get_lines` fails.
    /// - The requested index is out of range (no line returned).
    fn get_line(&self, idx: usize) -> color_eyre::Result<nvim_oxi::String>;

    fn set_text_at_cursor_pos(&mut self, text: &str);
}

impl BufferExt for Buffer {
    /// Get line.
    fn get_line(&self, idx: usize) -> color_eyre::Result<nvim_oxi::String> {
        self.get_lines(idx..=idx, true)?
            .next()
            .ok_or_else(|| eyre!("buffer line missing | idx={idx} buffer={self:#?}"))
    }

    /// Inserts `text` at the current cursor position in the active buffer.
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
            crate::oxi_ext::api::notify_error(&format!(
                "cannot set text in buffer | text={text:?} buffer={self:?} line_range={line_range:?} start_col={start_col:?} end_col={end_col:?} error={error:?}",
            ));
        }
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
            crate::oxi_ext::api::notify_error(&format!("cannot get cursor | window={cur_win:?} error={error:?}"));
        }) else {
            return None;
        };
        Some(Self { row, col })
    }
}
