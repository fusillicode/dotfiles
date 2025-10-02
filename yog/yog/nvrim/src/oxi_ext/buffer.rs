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
    /// Returns an error if:
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
        let cur_win = Window::current();
        let Ok((row, col)) = cur_win.get_cursor().inspect_err(|error| {
            crate::oxi_ext::api::notify_error(&format!("cannot get cursor | window={cur_win:?} error={error:?}"));
        }) else {
            return;
        };

        let row = row.saturating_sub(1);
        let line_range = row..row;
        let start_col = col;
        let end_col = col;
        let text = vec![text];

        if let Err(e) = self.set_text(line_range.clone(), start_col, end_col, text.clone()) {
            crate::oxi_ext::api::notify_error(&format!(
                "cannot set text in buffer | text={text:?} buffer={self:?} line_range={line_range:?} start_col={start_col:?} end_col={end_col:?} error={e:?}",
            ));
        }
    }
}
