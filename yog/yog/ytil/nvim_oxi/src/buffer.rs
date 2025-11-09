//! Buffer extension utilities (line access, cursor‑based insertion, cursor position model).
//!
//! Supplies [`BufferExt`] trait plus [`CursorPosition`] struct preserving raw Neovim coordinates for
//! consistent conversions at call sites.

use std::ops::RangeInclusive;

use color_eyre::eyre::eyre;
use nvim_oxi::api::Buffer;
use nvim_oxi::api::SuperIterator;
use nvim_oxi::api::Window;
use nvim_oxi::api::opts::OptionOptsBuilder;

/// Extension trait for [`Buffer`] to provide extra functionalities.
///
/// Provides focused helpers for line fetching and text insertion at the current
/// cursor position while surfacing Neovim errors via `notify_error`.
#[cfg_attr(any(test, feature = "mockall"), mockall::automock)]
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

    fn get_lines(
        &self,
        line_range: RangeInclusive<usize>,
        strict_indexing: bool,
    ) -> Result<Box<dyn SuperIterator<nvim_oxi::String>>, nvim_oxi::api::Error>;

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
    /// Retrieves text from the specified start position to end position, respecting the given boundary.
    ///
    /// # Arguments
    /// - `start` (lnum, col) 0-based starting line and column (column is byte offset).
    /// - `end` (`end_lnum`, `end_col`) 0-based ending line and column (inclusive; column is byte offset).
    /// - `boundary` [`TextBoundary`] specifying how to handle line boundaries.
    ///
    /// # Returns
    /// - `Ok(String)` with the extracted text, where lines are joined with "/n".
    ///
    /// # Errors
    /// - If substring extraction fails due to invalid indices.
    fn get_text_between(
        &self,
        start: (usize, usize),
        end: (usize, usize),
        boundary: TextBoundary,
    ) -> color_eyre::Result<String> {
        let (start_lnum, start_col) = start;
        let (end_lnum, end_col) = end;

        let lines = self.get_lines(start_lnum..=end_lnum, true)?;
        let last_line_idx = lines.len().saturating_sub(1);

        let mut out = String::new();
        for (line_idx, line) in lines.enumerate() {
            let line = line.to_string();
            let line_start_idx = boundary.get_line_start_idx(line_idx, start_col);
            let line_end_idx = boundary.get_line_end_idx(&line, line_idx, last_line_idx, end_col);
            let sub_line = line.get(line_start_idx..line_end_idx).ok_or_else(|| {
                eyre!(
                    "cannot extract substring from line | line={line:?} idx={line_idx} start_idx={line_start_idx} end_idx={line_end_idx}"
                )
            })?;
            out.push_str(sub_line);
            if line_idx != last_line_idx {
                out.push_str("/n")
            }
        }

        Ok(out)
    }

    /// Retrieves the buffer type via the `buftype` option.
    ///
    /// # Returns
    /// - `Ok(String)` The buffer type (e.g., `""` for normal, `"help"` for help buffers).
    ///
    /// # Errors
    /// - Propagates [`nvim_oxi::api::Error`] from the underlying option retrieval.
    fn get_buf_type(&self) -> Result<String, nvim_oxi::api::Error>;
}

#[derive(Default)]
pub enum TextBoundary {
    #[default]
    Exact,
    FromLineStart,
    ToLineEnd,
    FromLineStartToEnd,
}

impl TextBoundary {
    pub fn get_line_start_idx(&self, line_idx: usize, start_col: usize) -> usize {
        if line_idx != 0 {
            return 0;
        }
        match self {
            Self::FromLineStart | Self::FromLineStartToEnd => 0,
            Self::Exact | Self::ToLineEnd => start_col,
        }
    }

    pub fn get_line_end_idx(&self, line: &str, line_idx: usize, last_line_idx: usize, end_col: usize) -> usize {
        let line_len = line.len();
        if line_idx != last_line_idx {
            return line_len;
        }
        match self {
            Self::ToLineEnd | Self::FromLineStartToEnd => line_len,
            Self::Exact | Self::FromLineStart => end_col.min(line_len),
        }
    }
}

impl BufferExt for Buffer {
    /// Get line.
    fn get_line(&self, idx: usize) -> color_eyre::Result<nvim_oxi::String> {
        self.get_lines(idx..=idx, true)?
            .next()
            .ok_or_else(|| eyre!("buffer line missing | idx={idx} buffer={self:#?}"))
    }

    fn get_lines(
        &self,
        line_range: RangeInclusive<usize>,
        strict_indexing: bool,
    ) -> Result<Box<dyn SuperIterator<nvim_oxi::String>>, nvim_oxi::api::Error> {
        self.get_lines(line_range, strict_indexing)
            .map(|i| Box::new(i) as Box<dyn SuperIterator<nvim_oxi::String>>)
    }

    /// Insert text at cursor.
    fn set_text_at_cursor_pos(&mut self, text: &str) {
        let Some(cur_pos) = CursorPosition::get_current() else {
            return;
        };

        let row = cur_pos.row.saturating_sub(1);
        // TODO: must this be upper inclusive?
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

    fn get_buf_type(&self) -> Result<String, nvim_oxi::api::Error> {
        let opts = OptionOptsBuilder::default().buf(self.clone()).build();
        nvim_oxi::api::get_option_value::<String>("buftype", &opts)
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
    use mockall::predicate::*;

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

    #[test]
    fn get_text_between_single_line_exact() {
        let mock = mock_buffer(vec!["hello world".to_string()], 0, 0);
        let buffer = TestBuffer { mock };

        let result = buffer.get_text_between((0, 6), (0, 11), TextBoundary::Exact);

        assert2::let_assert!(Ok(value) = result);
        pretty_assertions::assert_eq!(value, "world");
    }

    #[test]
    fn get_text_between_single_line_from_line_start() {
        let mock = mock_buffer(vec!["hello world".to_string()], 0, 0);
        let buffer = TestBuffer { mock };

        let result = buffer.get_text_between((0, 6), (0, 11), TextBoundary::FromLineStart);

        assert2::let_assert!(Ok(value) = result);
        pretty_assertions::assert_eq!(value, "hello world");
    }

    #[test]
    fn get_text_between_single_line_to_line_end() {
        let mock = mock_buffer(vec!["hello world".to_string()], 0, 0);
        let buffer = TestBuffer { mock };

        let result = buffer.get_text_between((0, 0), (0, 5), TextBoundary::ToLineEnd);

        assert2::let_assert!(Ok(value) = result);
        pretty_assertions::assert_eq!(value, "hello world");
    }

    #[test]
    fn get_text_between_single_line_from_start_to_end() {
        let mock = mock_buffer(vec!["hello world".to_string()], 0, 0);
        let buffer = TestBuffer { mock };

        let result = buffer.get_text_between((0, 6), (0, 5), TextBoundary::FromLineStartToEnd);

        assert2::let_assert!(Ok(value) = result);
        pretty_assertions::assert_eq!(value, "hello world");
    }

    #[test]
    fn get_text_between_multiple_lines_exact() {
        let mock = mock_buffer(
            vec!["line1".to_string(), "line2".to_string(), "line3".to_string()],
            0,
            2,
        );
        let buffer = TestBuffer { mock };

        let result = buffer.get_text_between((0, 1), (2, 3), TextBoundary::Exact);

        assert2::let_assert!(Ok(value) = result);
        pretty_assertions::assert_eq!(value, "ine1/nline2/nlin");
    }

    #[test]
    fn get_text_between_multiple_lines_from_start_to_end() {
        let mock = mock_buffer(
            vec!["line1".to_string(), "line2".to_string(), "line3".to_string()],
            0,
            2,
        );
        let buffer = TestBuffer { mock };

        let result = buffer.get_text_between((0, 1), (2, 3), TextBoundary::FromLineStartToEnd);

        assert2::let_assert!(Ok(value) = result);
        pretty_assertions::assert_eq!(value, "line1/nline2/nline3");
    }

    #[test]
    fn get_text_between_multiple_lines_to_line_end() {
        let mock = mock_buffer(
            vec!["line1".to_string(), "line2".to_string(), "line3".to_string()],
            0,
            2,
        );
        let buffer = TestBuffer { mock };

        let result = buffer.get_text_between((0, 1), (2, 3), TextBoundary::ToLineEnd);

        assert2::let_assert!(Ok(value) = result);
        pretty_assertions::assert_eq!(value, "ine1/nline2/nline3");
    }

    #[test]
    fn get_text_between_error_out_of_bounds() {
        let mock = mock_buffer(vec!["hello".to_string()], 0, 0);
        let buffer = TestBuffer { mock };

        let result = buffer.get_text_between((0, 10), (0, 15), TextBoundary::Exact);

        assert2::let_assert!(Err(e) = result);
        pretty_assertions::assert_eq!(
            e.to_string(),
            r#"cannot extract substring from line | line="hello" idx=0 start_idx=10 end_idx=5"#
        );
    }

    fn mock_buffer(lines: Vec<String>, start_line: usize, end_line: usize) -> MockBufferExt {
        let mut mock = MockBufferExt::new();
        mock.expect_get_lines()
            .with(eq(start_line..=end_line), eq(true))
            .returning(move |_, _| {
                let lines: Vec<nvim_oxi::String> = lines.iter().map(|s| nvim_oxi::String::from(s.as_str())).collect();
                Ok(Box::new(lines.into_iter()) as Box<dyn SuperIterator<nvim_oxi::String>>)
            });
        mock
    }

    struct TestBuffer {
        mock: MockBufferExt,
    }

    impl BufferExt for TestBuffer {
        fn get_line(&self, _idx: usize) -> color_eyre::Result<nvim_oxi::String> {
            unimplemented!()
        }

        fn get_lines(
            &self,
            line_range: RangeInclusive<usize>,
            strict_indexing: bool,
        ) -> Result<Box<dyn SuperIterator<nvim_oxi::String>>, nvim_oxi::api::Error> {
            self.mock.get_lines(line_range, strict_indexing)
        }

        fn set_text_at_cursor_pos(&mut self, _text: &str) {
            unimplemented!()
        }

        fn get_buf_type(&self) -> Result<String, nvim_oxi::api::Error> {
            unimplemented!()
        }
    }
}

#[cfg(any(test, feature = "mockall"))]
pub mod mock {
    use nvim_oxi::api::SuperIterator;

    use super::*;

    pub struct MockBuffer {
        pub lines: Vec<String>,
        pub buf_type: String,
    }

    impl MockBuffer {
        pub fn new(lines: Vec<String>) -> Self {
            Self {
                lines,
                buf_type: "test".to_string(),
            }
        }

        pub fn with_buf_type(lines: Vec<String>, buf_type: &str) -> Self {
            Self {
                lines,
                buf_type: buf_type.to_string(),
            }
        }
    }

    impl BufferExt for MockBuffer {
        fn get_line(&self, _idx: usize) -> color_eyre::Result<nvim_oxi::String> {
            Ok(nvim_oxi::String::from(""))
        }

        fn get_lines(
            &self,
            line_range: std::ops::RangeInclusive<usize>,
            _strict_indexing: bool,
        ) -> Result<Box<dyn SuperIterator<nvim_oxi::String>>, nvim_oxi::api::Error> {
            let start = *line_range.start();
            let end = *line_range.end() + 1;
            let lines: Vec<nvim_oxi::String> = self.lines[start..end.min(self.lines.len())]
                .iter()
                .map(|s| nvim_oxi::String::from(s.as_str()))
                .collect();
            Ok(Box::new(lines.into_iter()) as Box<dyn SuperIterator<nvim_oxi::String>>)
        }

        fn set_text_at_cursor_pos(&mut self, _text: &str) {}

        fn get_buf_type(&self) -> Result<String, nvim_oxi::api::Error> {
            Ok(self.buf_type.clone())
        }
    }
}
