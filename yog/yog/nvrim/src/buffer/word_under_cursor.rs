//! Token classification under cursor (URL / file / directory / word).
//!
//! Retrieves current line + cursor column, extracts contiguous non‑whitespace token, classifies via
//! filesystem inspection or URL parsing, returning a tagged Lua table.

use std::process::Command;

use nvim_oxi::Object;
use nvim_oxi::conversion::ToObject;
use nvim_oxi::lua::Pushable;
use nvim_oxi::lua::ffi::State;
use nvim_oxi::serde::Serializer;
use serde::Serialize;
use url::Url;
use ytil_cmd::CmdExt as _;
use ytil_nvim_oxi::buffer::CursorPosition;

/// Retrieve and classify the non-whitespace token under the cursor in the current window.
///
/// Returns [`Option::None`] if the current line or cursor position cannot be obtained,
/// or if the cursor is on whitespace. On errors a notification is emitted to Nvim.
/// On success returns a classified [`WordUnderCursor`].
pub fn get(_: ()) -> Option<WordUnderCursor> {
    let cur_line = nvim_oxi::api::get_current_line()
        .inspect_err(|err| ytil_nvim_oxi::notify::error(format!("error getting current line | error={err:#?}")))
        .ok()?;
    let col = CursorPosition::get_current()?.col;
    get_word_at_index(&cur_line, col)
        .map(ToOwned::to_owned)
        .map(WordUnderCursor::from)
}

/// Classified representation of the token found under the cursor.
///
/// Used to distinguish between:
/// - URLs
/// - existing binary files
/// - existing text files
/// - existing directories
/// - plain tokens (fallback [`WordUnderCursor::Word`])
///
/// Serialized to Lua as a tagged table (`{ kind = "...", value = "..." }`).
#[derive(Serialize)]
#[serde(tag = "kind", content = "value")]
#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
pub enum WordUnderCursor {
    /// A string that successfully parsed as a [`Url`] via [`Url::parse`].
    Url(String),
    /// A filesystem path identified as a binary file by [`exec_file_cmd`].
    BinaryFile(String),
    /// A filesystem path identified as a text file by [`exec_file_cmd`].
    TextFile(TextFile),
    /// A filesystem path identified as a directory by [`exec_file_cmd`].
    Directory(String),
    /// A fallback plain token (word) when no more specific classification applied.
    Word(String),
}

#[derive(Serialize)]
#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
pub struct TextFile {
    pub path: String,
    pub lnum: i64,
    pub col: i64,
}

impl Pushable for WordUnderCursor {
    unsafe fn push(self, lstate: *mut State) -> Result<std::ffi::c_int, nvim_oxi::lua::Error> {
        unsafe {
            self.to_object()
                .map_err(nvim_oxi::lua::Error::push_error_from_err::<Self, _>)?
                .push(lstate)
        }
    }
}

impl ToObject for WordUnderCursor {
    fn to_object(self) -> Result<Object, nvim_oxi::conversion::Error> {
        self.serialize(Serializer::new()).map_err(Into::into)
    }
}

/// Classify a [`String`] captured under the cursor into a [`WordUnderCursor`].
///
/// 1. If it parses as a URL with [`Url::parse`], returns [`WordUnderCursor::Url`].
/// 2. Otherwise, invokes [`exec_file_cmd`] to check filesystem type.
/// 3. Falls back to [`WordUnderCursor::Word`] on errors or unknown kinds.
impl From<String> for WordUnderCursor {
    fn from(value: String) -> Self {
        if Url::parse(&value).is_ok() {
            return Self::Url(value);
        }

        let mut parts = value.split(':');
        let Some(maybe_path) = parts.next() else {
            return Self::Word(value);
        };

        let Ok(lnum) = parts
            .next()
            .map(str::parse)
            .transpose()
            .map(|x: Option<i64>| x.unwrap_or_default())
        else {
            return Self::Word(value);
        };

        let Ok(col) = parts
            .next()
            .map(str::parse)
            .transpose()
            .map(|x: Option<i64>| x.unwrap_or_default())
        else {
            return Self::Word(value);
        };

        match exec_file_cmd(maybe_path) {
            Ok(FileCmdOutput::BinaryFile(x)) => Self::BinaryFile(x),
            Ok(FileCmdOutput::TextFile(path)) => Self::TextFile(TextFile { path, lnum, col }),
            Ok(FileCmdOutput::Directory(x)) => Self::Directory(x),
            Ok(FileCmdOutput::NotFound(path) | FileCmdOutput::Unknown(path)) => Self::Word(path),
            Err(_) => Self::Word(value),
        }
    }
}

/// Execute the system `file -I` command for `path` and classify the MIME output
/// into a [`FileCmdOutput`].
///
/// Used to distinguish:
/// - directories
/// - text files
/// - binary files
/// - missing paths
/// - unknown types
///
/// # Errors
/// - launching or waiting on the `file` command fails
/// - the command exits with non-success
/// - standard output cannot be decoded as valid UTF-8
fn exec_file_cmd(path: &str) -> color_eyre::Result<FileCmdOutput> {
    let output = std::str::from_utf8(&Command::new("file").args([path, "-I"]).exec()?.stdout)?.to_lowercase();
    if output.contains(" inode/directory;") {
        return Ok(FileCmdOutput::Directory(path.to_owned()));
    }
    if output.contains(" text/plain;") || output.contains(" text/csv;") {
        return Ok(FileCmdOutput::TextFile(path.to_owned()));
    }
    if output.contains("application/") {
        return Ok(FileCmdOutput::BinaryFile(path.to_owned()));
    }
    if output.contains(" no such file or directory") {
        return Ok(FileCmdOutput::NotFound(path.to_owned()));
    }
    Ok(FileCmdOutput::Unknown(path.to_owned()))
}

/// Raw filesystem / MIME classification result returned by [`exec_file_cmd`].
#[derive(Serialize)]
pub enum FileCmdOutput {
    /// Path identified as a binary file.
    BinaryFile(String),
    /// Path identified as a text (plain / CSV) file.
    TextFile(String),
    /// Path identified as a directory.
    Directory(String),
    /// Path that does not exist.
    NotFound(String),
    /// Path whose type could not be determined.
    Unknown(String),
}

/// Find the non-whitespace token in the supplied string `s` containing the visual index `idx`.
///
/// Returns [`Option::None`] if:
/// - `idx` Is out of bounds.
/// - `idx` Does not point to a character boundary.
/// - The character at `idx` is whitespace
fn get_word_at_index(s: &str, idx: usize) -> Option<&str> {
    let byte_idx = convert_visual_to_byte_idx(s, idx)?;

    // If pointing to whitespace, no word.
    if s[byte_idx..].chars().next().is_some_and(char::is_whitespace) {
        return None;
    }

    // Scan split words and see which span contains `byte_idx`.
    let mut pos = 0;
    for word in s.split_ascii_whitespace() {
        let start = s[pos..].find(word)?.saturating_add(pos);
        let end = start.saturating_add(word.len());
        if (start..=end).contains(&byte_idx) {
            return Some(word);
        }
        pos = end;
    }
    None
}

/// Convert a visual (character) index into a byte index for the supplied string `s`.
///
/// Returns:
/// - [`Option::Some`] with the corresponding byte index (including `s.len()` for end-of-line)
/// - [`Option::None`] if `idx` is past the end
fn convert_visual_to_byte_idx(s: &str, idx: usize) -> Option<usize> {
    let mut chars_seen = 0usize;
    let mut byte_idx = None;
    for (b, _) in s.char_indices() {
        if chars_seen == idx {
            byte_idx = Some(b);
            break;
        }
        chars_seen = chars_seen.saturating_add(1);
    }
    if byte_idx.is_some() {
        return byte_idx;
    }
    if idx == chars_seen {
        return Some(s.len());
    }
    None
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use tempfile::NamedTempFile;
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn get_word_at_index_returns_word_inside_ascii_word() {
        let s = "open file.txt now";
        let idx = 7;
        assert_eq!(get_word_at_index(s, idx), Some("file.txt"));
    }

    #[test]
    fn get_word_at_index_returns_word_at_start_and_end_boundaries() {
        let s = "yes run main.rs";
        let idx_start = 8;
        let idx_last_inside = 14;
        assert_eq!(get_word_at_index(s, idx_start), Some("main.rs"));
        assert_eq!(get_word_at_index(s, idx_last_inside), Some("main.rs"));
    }

    #[test]
    fn get_word_at_index_returns_none_on_whitespace() {
        let s = "hello  world";
        assert_eq!(get_word_at_index(s, 5), None);
        assert_eq!(get_word_at_index(s, 6), None);
    }

    #[test]
    fn get_word_at_index_includes_punctuation_in_word() {
        let s = "print(arg)";
        let idx = 5;
        assert_eq!(get_word_at_index(s, idx), Some("print(arg)"));
    }

    #[test]
    fn get_word_at_index_returns_word_at_line_boundaries() {
        let s = "/usr/local/bin";
        assert_eq!(get_word_at_index(s, 0), Some("/usr/local/bin"));
        assert_eq!(get_word_at_index(s, 14), Some("/usr/local/bin"));
    }

    #[test]
    fn get_word_at_index_handles_utf8_boundaries_and_space() {
        let s = "αβ γ";
        assert_eq!(get_word_at_index(s, 0), Some("αβ"));
        assert_eq!(get_word_at_index(s, 1), Some("αβ"));
        assert_eq!(get_word_at_index(s, 4), Some("γ"));
        assert_eq!(get_word_at_index(s, 5), None);
    }

    #[test]
    fn get_word_at_index_returns_none_with_index_out_of_bounds() {
        let s = "abc";
        assert_eq!(get_word_at_index(s, 10), None);
    }

    #[test]
    fn word_under_cursor_from_valid_url_returns_url() {
        let input = "https://example.com".to_string();
        let result = WordUnderCursor::from(input.clone());
        assert_eq!(result, WordUnderCursor::Url(input));
    }

    #[test]
    fn word_under_cursor_from_invalid_url_plain_word_returns_word() {
        let input = "noturl".to_string();
        let result = WordUnderCursor::from(input.clone());
        assert_eq!(result, WordUnderCursor::Word(input));
    }

    #[test]
    fn word_under_cursor_from_path_to_text_file_returns_text_file() {
        let mut temp_file = NamedTempFile::new().unwrap();
        std::io::Write::write_all(&mut temp_file, b"hello world").unwrap();
        let path = temp_file.path().to_string_lossy().to_string();
        let result = WordUnderCursor::from(path.clone());
        assert_eq!(result, WordUnderCursor::TextFile(TextFile { path, lnum: 0, col: 0 }));
    }

    #[test]
    fn word_under_cursor_from_path_lnum_to_text_file_returns_text_file_with_lnum() {
        let mut temp_file = NamedTempFile::new().unwrap();
        std::io::Write::write_all(&mut temp_file, b"hello world").unwrap();
        let path = temp_file.path().to_string_lossy().to_string();
        let result = WordUnderCursor::from(format!("{path}:10"));
        assert_eq!(result, WordUnderCursor::TextFile(TextFile { path, lnum: 10, col: 0 }));
    }

    #[test]
    fn word_under_cursor_from_path_lnum_col_to_text_file_returns_text_file_with_lnum_col() {
        let mut temp_file = NamedTempFile::new().unwrap();
        std::io::Write::write_all(&mut temp_file, b"hello world").unwrap();
        let path = temp_file.path().to_string_lossy().to_string();
        let result = WordUnderCursor::from(format!("{path}:10:5"));
        assert_eq!(result, WordUnderCursor::TextFile(TextFile { path, lnum: 10, col: 5 }));
    }

    #[test]
    fn word_under_cursor_from_path_to_directory_returns_directory() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().to_string_lossy().to_string();
        let result = WordUnderCursor::from(path.clone());
        assert_eq!(result, WordUnderCursor::Directory(path));
    }

    #[test]
    fn word_under_cursor_from_path_to_binary_file_returns_binary_file() {
        let mut temp_file = NamedTempFile::new().unwrap();
        // Write some binary data
        std::io::Write::write_all(&mut temp_file, &[0, 1, 2, 255]).unwrap();
        let path = temp_file.path().to_string_lossy().to_string();
        let result = WordUnderCursor::from(path.clone());
        assert_eq!(result, WordUnderCursor::BinaryFile(path));
    }

    #[test]
    fn word_under_cursor_from_nonexistent_path_returns_word() {
        let path = "/nonexistent/path".to_string();
        let result = WordUnderCursor::from(path.clone());
        assert_eq!(result, WordUnderCursor::Word(path));
    }

    #[test]
    fn word_under_cursor_from_path_with_invalid_lnum_returns_word() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_string_lossy().to_string();
        let input = format!("{path}:invalid");
        let result = WordUnderCursor::from(input.clone());
        assert_eq!(result, WordUnderCursor::Word(input));
    }

    #[test]
    fn word_under_cursor_from_path_with_invalid_col_returns_word() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_string_lossy().to_string();
        let input = format!("{path}:10:invalid");
        let result = WordUnderCursor::from(input.clone());
        assert_eq!(result, WordUnderCursor::Word(input));
    }

    #[test]
    fn word_under_cursor_from_path_lnum_col_extra_ignores_extra() {
        let mut temp_file = NamedTempFile::new().unwrap();
        std::io::Write::write_all(&mut temp_file, b"hello world").unwrap();
        let path = temp_file.path().to_string_lossy().to_string();
        let result = WordUnderCursor::from(format!("{path}:10:5:extra"));
        assert_eq!(result, WordUnderCursor::TextFile(TextFile { path, lnum: 10, col: 5 }));
    }
}
