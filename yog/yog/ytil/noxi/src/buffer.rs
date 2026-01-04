//! Buffer extension utilities like line access, cursor‑based insertion, cursor position model, etc.

use std::fmt::Debug;
use std::ops::RangeInclusive;
use std::path::Path;
use std::path::PathBuf;

use color_eyre::eyre::Context;
use color_eyre::eyre::eyre;
use nvim_oxi::Array;
use nvim_oxi::api::Buffer;
use nvim_oxi::api::SuperIterator;
use nvim_oxi::api::Window;
use nvim_oxi::api::opts::OptionOptsBuilder;

use crate::visual_selection::Selection;

/// Extension trait for [`Buffer`] to provide extra functionalities.
///
/// Provides focused helpers for line fetching and text insertion at the current
/// cursor position while surfacing Nvim errors via `notify_error`.
#[cfg_attr(any(test, feature = "mockall"), mockall::automock)]
pub trait BufferExt: Debug {
    /// Fetch a single line from a [`Buffer`] by 0-based index.
    ///
    /// Returns a [`color_eyre::Result`] with the line as [`nvim_oxi::String`].
    /// Errors if the line does not exist at `idx`.
    ///
    /// # Errors
    /// - Fetching the line via `nvim_buf_get_lines` fails.
    /// - The requested index is out of range (no line returned).
    fn get_line(&self, idx: usize) -> color_eyre::Result<nvim_oxi::String>;

    /// Retrieves a range of lines from the buffer.
    ///
    /// # Errors
    /// - If `strict_indexing` is true and the range is out of bounds.
    /// - If the Nvim API call to fetch lines fails.
    ///
    /// # Rationale
    /// This is a thin wrapper around [`nvim_oxi::api::Buffer::get_lines`] to enable unit testing of the default trait
    /// method [`BufferExt::get_text_between`].
    fn get_lines(
        &self,
        line_range: RangeInclusive<usize>,
        strict_indexing: bool,
    ) -> Result<Box<dyn SuperIterator<nvim_oxi::String>>, nvim_oxi::api::Error>;

    /// Get text from a [`nvim_oxi::api::Buffer`].
    ///
    /// Retrieves text from the specified start position to end position, respecting the given boundary.
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
                out.push_str("/n");
            }
        }

        Ok(out)
    }

    /// Retrieves the buffer type via the `buftype` option.
    ///
    /// Queries Nvim for the buffer type option and returns the value.
    /// Errors are handled internally by notifying Nvim and converting to `None`.
    ///
    /// # Rationale
    /// Errors are notified directly to Nvim because this is the behavior wanted in all cases.
    fn get_buf_type(&self) -> Option<String>;

    fn get_channel(&self) -> Option<u32>;

    /// Inserts `text` at the current cursor position in the active buffer.
    ///
    /// Obtains the current [`CursorPosition`], converts the 1-based row to 0-based
    /// for Nvim's `set_text` call, and inserts `text` without replacing existing
    /// content (`start_col` == `end_col`). Errors are reported via `notify_error`.
    /// Silently returns if cursor position cannot be fetched.
    fn set_text_at_cursor_pos(&mut self, text: &str);

    fn is_terminal(&self) -> bool {
        self.get_buf_type().is_some_and(|bt| bt == "terminal")
    }

    fn send_command(&self, cmd: &str) -> Option<()> {
        let channel_id = self.get_channel()?;

        nvim_oxi::api::chan_send(channel_id, cmd).inspect_err(|err|{
            crate::notify::error(format!(
                "error sending command to buffer | command={cmd:?} buffer={self:?} channel_id={channel_id} error={err:?}"
            ));
        }).ok()?;

        Some(())
    }

    /// Retrieves the process ID associated with the buffer.
    ///
    /// # Errors
    /// - If the buffer name cannot be retrieved.
    /// - If the buffer is a terminal but the name format is invalid.
    /// - If the Neovim `getpid` function call fails.
    fn get_pid(&self) -> color_eyre::Result<String>;
}

/// Defines boundaries for text selection within lines.
///
/// # Rationale
/// Used in [`BufferExt::get_text_between`] to specify how the start and end positions
/// should be interpreted relative to line boundaries.
#[derive(Default)]
pub enum TextBoundary {
    /// Exact column positions are used as-is.
    #[default]
    Exact,
    /// Selection starts from the beginning of the line.
    FromLineStart,
    /// Selection ends at the specified line ending column.
    ToLineEnd,
    /// Selection spans from the start of the line to the end of the line.
    FromLineStartToEnd,
}

impl TextBoundary {
    /// Computes the starting column index for text selection based on the boundary type.
    pub const fn get_line_start_idx(&self, line_idx: usize, start_col: usize) -> usize {
        if line_idx != 0 {
            return 0;
        }
        match self {
            Self::FromLineStart | Self::FromLineStartToEnd => 0,
            Self::Exact | Self::ToLineEnd => start_col,
        }
    }

    /// Computes the ending column index for text selection based on the boundary type.
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
    fn get_line(&self, idx: usize) -> color_eyre::Result<nvim_oxi::String> {
        self.get_lines(idx..=idx, true)
            .wrap_err_with(|| format!("error getting buffer line at index | idx={idx} buffer={self:?}"))?
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

    fn set_text_at_cursor_pos(&mut self, text: &str) {
        let Some(cur_pos) = CursorPosition::get_current() else {
            return;
        };

        let row = cur_pos.row.saturating_sub(1);
        let line_range = row..=row;
        let start_col = cur_pos.col;
        let end_col = cur_pos.col;

        if let Err(err) = self.set_text(line_range.clone(), start_col, end_col, vec![text]) {
            crate::notify::error(format!(
                "error setting text in buffer | text={text:?} buffer={self:?} line_range={line_range:?} start_col={start_col:?} end_col={end_col:?} error={err:#?}",
            ));
        }
    }

    fn get_buf_type(&self) -> Option<String> {
        let opts = OptionOptsBuilder::default().buf(self.clone()).build();
        nvim_oxi::api::get_option_value::<String>("buftype", &opts)
            .inspect_err(|err| {
                crate::notify::error(format!(
                    "error getting buftype of buffer | buffer={self:#?} error={err:?}"
                ));
            })
            .ok()
    }

    fn get_channel(&self) -> Option<u32> {
        let opts = OptionOptsBuilder::default().buf(self.clone()).build();
        nvim_oxi::api::get_option_value::<u32>("channel", &opts)
            .inspect_err(|err| {
                crate::notify::error(format!(
                    "error getting channel of buffer | buffer={self:#?} error={err:?}"
                ));
            })
            .ok()
    }

    fn get_pid(&self) -> color_eyre::Result<String> {
        let buf_name = self
            .get_name()
            .wrap_err_with(|| eyre!("error getting name of buffer | buffer={self:#?}"))
            .map(|s| s.to_string_lossy().to_string())?;

        if buf_name.starts_with("term://") {
            let (_, pid_cmd) = buf_name.rsplit_once("//").ok_or_else(|| {
                eyre!("error getting pid and cmd from buffer name | buffer={self:?} buffer_name={buf_name:?}")
            })?;
            let (pid, _) = pid_cmd
                .rsplit_once(':')
                .ok_or_else(|| eyre!("error getting pid from buffer name| buffer={self:?} buffer_name={buf_name:?}"))?;
            return Ok(pid.to_owned());
        }

        let pid = nvim_oxi::api::call_function::<_, i32>("getpid", Array::new())
            .wrap_err_with(|| eyre!("error getting pid of buffer | buffer={self:#?}"))?;

        Ok(pid.to_string())
    }
}

/// Represents the current cursor coordinates in the active [`Window`].
///
/// Row is 1-based (Nvim convention) and column is 0-based (byte index inside
/// the line per Nvim API). These are kept verbatim to avoid off-by-one bugs.
/// Call sites converting to Rust slice indices subtract 1 from `row` as needed.
///
/// # Assumptions
/// - Constructed through [`CursorPosition::get_current`]; manual construction should respect coordinate conventions.
///
/// # Rationale
/// Preserving raw Nvim values centralizes conversion logic at usage points
/// (e.g. buffer line indexing) instead of embedding heuristics here.
#[derive(Debug)]
pub struct CursorPosition {
    pub row: usize,
    pub col: usize,
}

impl CursorPosition {
    /// Obtains the current cursor position from the active [`Window`].
    ///
    /// Queries Nvim for the (row, col) of the active window cursor and returns a
    /// [`CursorPosition`] reflecting those raw coordinates.
    ///
    /// # Assumptions
    /// - Row is 1-based (Nvim convention); column is 0-based. Callers needing 0-based row for Rust indexing must
    ///   subtract 1 explicitly.
    /// - The active window is the intended source of truth for cursor location.
    ///
    /// # Rationale
    /// Returning `Option` (instead of `Result`) simplifies common call sites that
    /// treat absence as a soft failure (e.g. skipping an insertion). Detailed
    /// error context is still surfaced to the user through `notify_error`.
    pub fn get_current() -> Option<Self> {
        let cur_win = Window::current();
        cur_win
            .get_cursor()
            .map(|(row, col)| Self { row, col })
            .inspect_err(|err| {
                crate::notify::error(format!(
                    "error getting cursor from current window | window={cur_win:?} error={err:#?}"
                ));
            })
            .ok()
    }

    /// Returns 1-based column index for rendering purposes.
    ///
    /// Converts the raw 0-based Nvim column stored in [`CursorPosition::col`] into a
    /// human-friendly 1-based column suitable for statusline / UI output.
    ///
    /// # Assumptions
    /// - [`CursorPosition::col`] is the unmodified 0-based byte offset provided by Nvim.
    ///
    /// # Rationale
    /// Nvim exposes a 0-based column while rows are 1-based. Normalizing to 1-based for
    /// display avoids mixed-base confusion in user-facing components (e.g. status line) and
    /// clarifies intent at call sites.
    ///
    /// # Performance
    /// Constant time. Uses `saturating_add` defensively (overflow is unrealistic given line length).
    pub const fn adjusted_col(&self) -> usize {
        self.col.saturating_add(1)
    }
}

/// Creates a new listed, not scratch, buffer.
///
/// Errors are reported to Nvim via [`crate::notify::error`].
pub fn create() -> Option<Buffer> {
    nvim_oxi::api::create_buf(true, false)
        .inspect_err(|err| crate::notify::error(format!("error creating buffer | error={err:?}")))
        .ok()
}

/// Retrieves the alternate buffer or creates a new one if none exists.
///
/// The alternate buffer is the buffer previously visited, accessed via Nvim's "#" register.
/// If no alternate buffer exists (bufnr("#") < 0), a new buffer is created.
///
/// # Errors
/// - Retrieving the alternate buffer fails (notified via [`crate::notify::error`]).
/// - Creating a new buffer fails (falls back to [`create`]).
pub fn get_alternate_or_new() -> Option<Buffer> {
    let alt_buf_id = nvim_oxi::api::call_function::<_, i32>("bufnr", ("#",))
        .inspect(|err| {
            crate::notify::error(format!("error getting alternate buffer | error={err:?}"));
        })
        .ok()?;

    if alt_buf_id < 0 {
        return create();
    }
    Some(Buffer::from(alt_buf_id))
}

/// Sets the specified buffer as the current buffer in the active window.
///
/// # Errors
/// - Setting the current buffer fails (notified via [`crate::notify::error`]).
pub fn set_current(buf: &Buffer) -> Option<()> {
    nvim_oxi::api::set_current_buf(buf)
        .inspect_err(|err| {
            crate::notify::error(format!("error setting current buffer | buffer={buf:?} error={err:?}"));
        })
        .ok()?;
    Some(())
}

/// Opens a file in the editor and positions the cursor at the specified line and column.
///
/// # Errors
/// - If execution of "edit" command via [`crate::common::exec_vim_cmd`] fails.
/// - If setting the cursor position via [`Window::set_cursor`] fails.
///
/// # Rationale
/// Executes two Neovim commands, one to open the file and one to set the cursor because it doesn't
/// seems possible to execute a command line "edit +call\n cursor(LNUM, COL)".
pub fn open<T: AsRef<Path>>(path: T, line: Option<usize>, col: Option<usize>) -> color_eyre::Result<()> {
    crate::common::exec_vim_cmd("edit", Some(&[path.as_ref().display().to_string()]))?;
    Window::current().set_cursor(line.unwrap_or_default(), col.unwrap_or_default())?;
    Ok(())
}

/// Replaces the text in the specified `selection` with the `replacement` lines.
///
/// Calls Nvim's `set_text` with the selection's line range and column positions,
/// replacing the selected content with the provided lines.
///
/// Errors are reported via [`crate::notify::error`] with details about the selection
/// boundaries and error.
pub fn replace_text_and_notify_if_error<Line, Lines>(selection: &Selection, replacement: Lines)
where
    Lines: IntoIterator<Item = Line>,
    Line: Into<nvim_oxi::String>,
{
    if let Err(err) = Buffer::from(selection.buf_id()).set_text(
        selection.line_range(),
        selection.start().col,
        selection.end().col,
        replacement,
    ) {
        crate::notify::error(format!(
            "error setting lines of buffer | start={:#?} end={:#?} error={err:#?}",
            selection.start(),
            selection.end()
        ));
    }
}

/// Retrieves the relative path of the given buffer from the current working directory.
///
/// Attempts to strip the current working directory prefix from the buffer's absolute path.
/// If the buffer path does not start with the cwd, returns the absolute path as-is.
///
/// # Errors
/// Errors (e.g., cannot get cwd or buffer name) are notified to Nvim but not propagated.
pub fn get_relative_path_to_cwd(current_buffer: &Buffer) -> Option<PathBuf> {
    let cwd = nvim_oxi::api::call_function::<_, String>("getcwd", Array::new())
        .inspect_err(|err| {
            crate::notify::error(format!("error getting cwd | error={err:#?}"));
        })
        .ok()?;

    let current_buffer_path = get_absolute_path(Some(current_buffer))?.display().to_string();

    Some(PathBuf::from(
        current_buffer_path.strip_prefix(&cwd).unwrap_or(&current_buffer_path),
    ))
}

/// Retrieves the absolute path of the specified buffer.
///
/// # Errors
/// Errors are logged internally but do not propagate; the function returns [`None`] on failure.
///
/// # Assumptions
/// Assumes that the buffer's name represents a valid path.
pub fn get_absolute_path(buffer: Option<&Buffer>) -> Option<PathBuf> {
    let path = buffer?
        .get_name()
        .map(|s| s.to_string_lossy().to_string())
        .inspect_err(|err| {
            crate::notify::error(format!(
                "error getting buffer absolute path | buffer={buffer:#?} error={err:#?}"
            ));
        })
        .ok();

    if path.as_ref().is_some_and(String::is_empty) {
        return None;
    }

    path.map(PathBuf::from)
}

pub fn get_current_line() -> Option<String> {
    nvim_oxi::api::get_current_line()
        .inspect_err(|err| crate::notify::error(format!("error getting current line | error={err}")))
        .ok()
}

#[cfg(test)]
mod tests {
    use mockall::predicate::*;
    use rstest::rstest;

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
    fn buffer_ext_get_text_between_single_line_exact() {
        let mock = mock_buffer(vec!["hello world".to_string()], 0, 0);
        let buffer = TestBuffer { mock };

        let result = buffer.get_text_between((0, 6), (0, 11), TextBoundary::Exact);

        assert2::let_assert!(Ok(value) = result);
        pretty_assertions::assert_eq!(value, "world");
    }

    #[test]
    fn buffer_ext_get_text_between_single_line_from_line_start() {
        let mock = mock_buffer(vec!["hello world".to_string()], 0, 0);
        let buffer = TestBuffer { mock };

        let result = buffer.get_text_between((0, 6), (0, 11), TextBoundary::FromLineStart);

        assert2::let_assert!(Ok(value) = result);
        pretty_assertions::assert_eq!(value, "hello world");
    }

    #[test]
    fn buffer_ext_get_text_between_single_line_to_line_end() {
        let mock = mock_buffer(vec!["hello world".to_string()], 0, 0);
        let buffer = TestBuffer { mock };

        let result = buffer.get_text_between((0, 0), (0, 5), TextBoundary::ToLineEnd);

        assert2::let_assert!(Ok(value) = result);
        pretty_assertions::assert_eq!(value, "hello world");
    }

    #[test]
    fn buffer_ext_get_text_between_single_line_from_start_to_end() {
        let mock = mock_buffer(vec!["hello world".to_string()], 0, 0);
        let buffer = TestBuffer { mock };

        let result = buffer.get_text_between((0, 6), (0, 5), TextBoundary::FromLineStartToEnd);

        assert2::let_assert!(Ok(value) = result);
        pretty_assertions::assert_eq!(value, "hello world");
    }

    #[test]
    fn buffer_ext_get_text_between_multiple_lines_exact() {
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
    fn buffer_ext_get_text_between_multiple_lines_from_start_to_end() {
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
    fn buffer_ext_get_text_between_multiple_lines_to_line_end() {
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
    fn buffer_ext_get_text_between_error_out_of_bounds() {
        let mock = mock_buffer(vec!["hello".to_string()], 0, 0);
        let buffer = TestBuffer { mock };

        let result = buffer.get_text_between((0, 10), (0, 15), TextBoundary::Exact);

        assert2::let_assert!(Err(err) = result);
        pretty_assertions::assert_eq!(
            err.to_string(),
            r#"cannot extract substring from line | line="hello" idx=0 start_idx=10 end_idx=5"#
        );
    }

    #[rstest]
    #[case::exact_non_zero_line_idx(TextBoundary::Exact, 1, 5, 0)]
    #[case::to_line_end_non_zero_line_idx(TextBoundary::ToLineEnd, 1, 5, 0)]
    #[case::from_line_start_non_zero_line_idx(TextBoundary::FromLineStart, 1, 5, 0)]
    #[case::from_line_start_to_end_non_zero_line_idx(TextBoundary::FromLineStartToEnd, 1, 5, 0)]
    #[case::exact_zero_line_idx(TextBoundary::Exact, 0, 5, 5)]
    #[case::to_line_end_zero_line_idx(TextBoundary::ToLineEnd, 0, 5, 5)]
    #[case::from_line_start_zero_line_idx(TextBoundary::FromLineStart, 0, 5, 0)]
    #[case::from_line_start_to_end_zero_line_idx(TextBoundary::FromLineStartToEnd, 0, 5, 0)]
    fn text_boundary_get_line_start_idx(
        #[case] boundary: TextBoundary,
        #[case] line_idx: usize,
        #[case] start_col: usize,
        #[case] expected: usize,
    ) {
        pretty_assertions::assert_eq!(boundary.get_line_start_idx(line_idx, start_col), expected);
    }

    #[rstest]
    #[case::exact_line_idx_not_last(TextBoundary::Exact, "hello", 0, 1, 3, 5)]
    #[case::exact_line_idx_is_last(TextBoundary::Exact, "hello", 1, 1, 3, 3)]
    #[case::exact_end_col_greater_than_line_len(TextBoundary::Exact, "hi", 0, 0, 5, 2)]
    #[case::from_line_start_line_idx_is_last(TextBoundary::FromLineStart, "hello", 1, 1, 3, 3)]
    #[case::to_line_end_line_idx_is_last(TextBoundary::ToLineEnd, "hello", 1, 1, 3, 5)]
    #[case::from_line_start_to_end_line_idx_is_last(TextBoundary::FromLineStartToEnd, "hello", 1, 1, 3, 5)]
    fn text_boundary_get_line_end_idx(
        #[case] boundary: TextBoundary,
        #[case] line: &str,
        #[case] line_idx: usize,
        #[case] last_line_idx: usize,
        #[case] end_col: usize,
        #[case] expected: usize,
    ) {
        pretty_assertions::assert_eq!(
            boundary.get_line_end_idx(line, line_idx, last_line_idx, end_col),
            expected
        );
    }

    #[allow(clippy::needless_collect)]
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

    #[derive(Debug)]
    struct TestBuffer {
        mock: MockBufferExt,
    }

    impl BufferExt for TestBuffer {
        fn get_line(&self, _idx: usize) -> color_eyre::Result<nvim_oxi::String> {
            Ok("".into())
        }

        fn get_lines(
            &self,
            line_range: RangeInclusive<usize>,
            strict_indexing: bool,
        ) -> Result<Box<dyn SuperIterator<nvim_oxi::String>>, nvim_oxi::api::Error> {
            self.mock.get_lines(line_range, strict_indexing)
        }

        fn set_text_at_cursor_pos(&mut self, _text: &str) {}

        fn get_buf_type(&self) -> Option<String> {
            None
        }

        fn get_channel(&self) -> Option<u32> {
            None
        }

        fn send_command(&self, _cmd: &str) -> Option<()> {
            None
        }

        fn get_pid(&self) -> color_eyre::Result<String> {
            Ok("42".to_owned())
        }
    }
}

#[cfg(any(test, feature = "mockall"))]
pub mod mock {
    use nvim_oxi::api::SuperIterator;

    use crate::buffer::BufferExt;

    #[derive(Debug)]
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
            Ok("".into())
        }

        #[allow(clippy::needless_collect)]
        fn get_lines(
            &self,
            line_range: std::ops::RangeInclusive<usize>,
            _strict_indexing: bool,
        ) -> Result<Box<dyn SuperIterator<nvim_oxi::String>>, nvim_oxi::api::Error> {
            let start = *line_range.start();
            let end = line_range.end().saturating_add(1);
            let lines: Vec<nvim_oxi::String> = self
                .lines
                .get(start..end.min(self.lines.len()))
                .unwrap_or(&[])
                .iter()
                .map(|s| nvim_oxi::String::from(s.as_str()))
                .collect();
            Ok(Box::new(lines.into_iter()) as Box<dyn SuperIterator<nvim_oxi::String>>)
        }

        fn set_text_at_cursor_pos(&mut self, _text: &str) {}

        fn get_buf_type(&self) -> Option<String> {
            Some(self.buf_type.clone())
        }

        fn get_channel(&self) -> Option<u32> {
            None
        }

        fn send_command(&self, _cmd: &str) -> Option<()> {
            None
        }

        fn get_pid(&self) -> color_eyre::Result<String> {
            Ok("42".to_owned())
        }
    }
}
