#![allow(dead_code, unused_variables)]

//! Token classification under cursor (URL / file / directory / word).
//!
//! Retrieves current line + cursor column, extracts contiguous non‑whitespace token, classifies via
//! filesystem inspection or URL parsing, returning a tagged Lua table.

use std::process::Command;

use nvim_oxi::Object;
use nvim_oxi::conversion::ToObject;
use nvim_oxi::lua::ffi::State;
use nvim_oxi::serde::Serializer;
use serde::Serialize;
use url::Url;
use ytil_cmd::CmdExt as _;
use ytil_noxi::buffer::BufferExt;
use ytil_noxi::buffer::CursorPosition;

/// Retrieve and classify the non-whitespace token under the cursor in the current window.
///
/// Returns [`Option::None`] if the current line or cursor position cannot be obtained,
/// or if the cursor is on whitespace. On errors a notification is emitted to Nvim.
/// On success returns a classified [`WordUnderCursor`].
pub fn get(_: ()) -> Option<WordUnderCursor> {
    let current_buffer = nvim_oxi::api::get_current_buf();

    let cursor_pos = CursorPosition::get_current()?;
    let cursor_pos_col = cursor_pos.col;
    let cursor_pos_row = cursor_pos.row - 1;

    let mut rev_chars = vec![];

    // Backward loop
    'outer: for row_idx in (0..=cursor_pos_row).rev() {
        let current_row = current_buffer.get_line(row_idx).unwrap().to_string_lossy().to_string();

        for (char_idx, current_char) in current_row.char_indices().rev() {
            if row_idx == cursor_pos_row && char_idx > cursor_pos_col {
                continue;
            }
            if current_char.is_ascii_whitespace() {
                break 'outer;
            }
            rev_chars.push(current_char);
        }
    }

    let mut out: String = rev_chars.into_iter().rev().collect();

    if !out.is_empty() {
        // Fwd loop - using usize::MAX to avoid asking for actual window height (i.e. window max row)
        'outer: for row_idx in cursor_pos_row..usize::MAX {
            let current_row = current_buffer.get_line(row_idx).unwrap().to_str().unwrap().to_owned();

            for (char_idx, current_char) in current_row.char_indices() {
                if row_idx == cursor_pos_row && char_idx <= cursor_pos_col {
                    continue;
                }
                if current_char.is_ascii_whitespace() {
                    break 'outer;
                }
                out.push(current_char);
            }
        }
    }

    nvim_oxi::dbg!(&out);

    // TODO: switch back to from
    Some(WordUnderCursor::Word(out))
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
#[derive(Debug, Serialize)]
#[serde(tag = "kind", content = "value")]
#[cfg_attr(test, derive(Eq, PartialEq))]
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

/// Represents a text file with a specific cursor position for navigation.
#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(Eq, PartialEq))]
pub struct TextFile {
    /// The filesystem path to the text file.
    pub path: String,
    /// The 1-based line number within the file.
    pub lnum: i64,
    /// The 1-based column number within the line.
    pub col: i64,
}

impl nvim_oxi::lua::Pushable for WordUnderCursor {
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
    use rstest::*;
    #[cfg(not(feature = "ci"))]
    use tempfile::NamedTempFile;
    #[cfg(not(feature = "ci"))]
    use tempfile::TempDir;

    use super::*;

    #[rstest]
    #[case("open file.txt now", 7, Some("file.txt"))]
    #[case("yes run main.rs", 8, Some("main.rs"))]
    #[case("yes run main.rs", 14, Some("main.rs"))]
    #[case("hello  world", 5, None)]
    #[case("hello  world", 6, None)]
    #[case("/usr/local/bin", 0, Some("/usr/local/bin"))]
    #[case("/usr/local/bin", 14, Some("/usr/local/bin"))]
    #[case("print(arg)", 5, Some("print(arg)"))]
    #[case("abc", 10, None)]
    #[case("αβ γ", 0, Some("αβ"))]
    #[case("αβ γ", 1, Some("αβ"))]
    #[case("αβ γ", 4, Some("γ"))]
    #[case("αβ γ", 5, None)]
    #[case("hello\nworld", 0, Some("hello"))]
    #[case("hello\nworld", 6, Some("world"))]
    #[case("hello\nworld", 5, None)]
    #[case("hello\n\nworld", 5, None)]
    #[case("hello\n\nworld", 6, None)]
    fn get_word_at_index_scenarios(#[case] s: &str, #[case] idx: usize, #[case] expected: Option<&str>) {
        pretty_assertions::assert_eq!(get_word_at_index(s, idx), expected);
    }

    // Tests are skipped in CI because [`WordUnderCursor::from`] calls `file` command and that
    // behaves differently based on the platform (e.g. macOS vs Linux)

    #[test]
    #[cfg(not(feature = "ci"))]
    fn word_under_cursor_from_valid_url_returns_url() {
        let input = "https://example.com".to_string();
        let result = WordUnderCursor::from(input.clone());
        pretty_assertions::assert_eq!(result, WordUnderCursor::Url(input));
    }

    #[test]
    #[cfg(not(feature = "ci"))]
    fn word_under_cursor_from_invalid_url_plain_word_returns_word() {
        let input = "noturl".to_string();
        let result = WordUnderCursor::from(input.clone());
        pretty_assertions::assert_eq!(result, WordUnderCursor::Word(input));
    }

    #[test]
    #[cfg(not(feature = "ci"))]
    fn word_under_cursor_from_path_to_text_file_returns_text_file() {
        let mut temp_file = NamedTempFile::new().unwrap();
        std::io::Write::write_all(&mut temp_file, b"hello world").unwrap();
        let path = temp_file.path().to_string_lossy().to_string();
        let result = WordUnderCursor::from(path.clone());
        pretty_assertions::assert_eq!(result, WordUnderCursor::TextFile(TextFile { path, lnum: 0, col: 0 }));
    }

    #[test]
    #[cfg(not(feature = "ci"))]
    fn word_under_cursor_from_path_lnum_to_text_file_returns_text_file_with_lnum() {
        let mut temp_file = NamedTempFile::new().unwrap();
        std::io::Write::write_all(&mut temp_file, b"hello world").unwrap();
        let path = temp_file.path().to_string_lossy().to_string();
        let result = WordUnderCursor::from(format!("{path}:10"));
        pretty_assertions::assert_eq!(result, WordUnderCursor::TextFile(TextFile { path, lnum: 10, col: 0 }));
    }

    #[test]
    #[cfg(not(feature = "ci"))]
    fn word_under_cursor_from_path_lnum_col_to_text_file_returns_text_file_with_lnum_col() {
        let mut temp_file = NamedTempFile::new().unwrap();
        std::io::Write::write_all(&mut temp_file, b"hello world").unwrap();
        let path = temp_file.path().to_string_lossy().to_string();
        let result = WordUnderCursor::from(format!("{path}:10:5"));
        pretty_assertions::assert_eq!(result, WordUnderCursor::TextFile(TextFile { path, lnum: 10, col: 5 }));
    }

    #[test]
    #[cfg(not(feature = "ci"))]
    fn word_under_cursor_from_path_to_directory_returns_directory() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().to_string_lossy().to_string();
        let result = WordUnderCursor::from(path.clone());
        pretty_assertions::assert_eq!(result, WordUnderCursor::Directory(path));
    }

    #[test]
    #[cfg(not(feature = "ci"))]
    fn word_under_cursor_from_path_to_binary_file_returns_binary_file() {
        let mut temp_file = NamedTempFile::new().unwrap();
        // Write some binary data
        std::io::Write::write_all(&mut temp_file, &[0, 1, 2, 255]).unwrap();
        let path = temp_file.path().to_string_lossy().to_string();
        let result = WordUnderCursor::from(path.clone());
        pretty_assertions::assert_eq!(result, WordUnderCursor::BinaryFile(path));
    }

    #[test]
    #[cfg(not(feature = "ci"))]
    fn word_under_cursor_from_nonexistent_path_returns_word() {
        let path = "/nonexistent/path".to_string();
        let result = WordUnderCursor::from(path.clone());
        pretty_assertions::assert_eq!(result, WordUnderCursor::Word(path));
    }

    #[test]
    #[cfg(not(feature = "ci"))]
    fn word_under_cursor_from_path_with_invalid_lnum_returns_word() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_string_lossy().to_string();
        let input = format!("{path}:invalid");
        let result = WordUnderCursor::from(input.clone());
        pretty_assertions::assert_eq!(result, WordUnderCursor::Word(input));
    }

    #[test]
    #[cfg(not(feature = "ci"))]
    fn word_under_cursor_from_path_with_invalid_col_returns_word() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_string_lossy().to_string();
        let input = format!("{path}:10:invalid");
        let result = WordUnderCursor::from(input.clone());
        pretty_assertions::assert_eq!(result, WordUnderCursor::Word(input));
    }

    #[test]
    #[cfg(not(feature = "ci"))]
    fn word_under_cursor_from_path_lnum_col_extra_ignores_extra() {
        let mut temp_file = NamedTempFile::new().unwrap();
        std::io::Write::write_all(&mut temp_file, b"hello world").unwrap();
        let path = temp_file.path().to_string_lossy().to_string();
        let result = WordUnderCursor::from(format!("{path}:10:5:extra"));
        pretty_assertions::assert_eq!(result, WordUnderCursor::TextFile(TextFile { path, lnum: 10, col: 5 }));
    }
}
