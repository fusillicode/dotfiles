use std::process::Command;

use nvim_oxi::Object;
use nvim_oxi::api::Window;
use nvim_oxi::conversion::ToObject;
use nvim_oxi::lua::ffi::State;
use nvim_oxi::serde::Serializer;
use serde::Serialize;
use url::Url;
use cmd::CmdExt as _;

/// Gets the non-whitespace "word" under the cursor in the current window.
/// On failure returns [`Option::None`] and notifies an error to Nvim.
/// If on a whitespace returns an [`Option::None`].
pub fn get(_: ()) -> Option<WordUnderCursor> {
    let cur_win = Window::current();
    let cur_line = nvim_oxi::api::get_current_line()
        .inspect_err(|e| crate::oxi_ext::notify_error(&format!("cannot get current line: {e:#?}")))
        .ok()?;
    let (_, col) = cur_win
        .get_cursor()
        .inspect_err(|e| crate::oxi_ext::notify_error(&format!("cannot get cursor: {e:#?}")))
        .ok()?;
    get_word_at_index(&cur_line, col)
        .map(ToOwned::to_owned)
        .map(WordUnderCursor::from)
}

/// Classified representation of the "word" found under the cursor.
///
/// Used to distinguish between:
/// - URLs
/// - existing binary files
/// - existing text files
/// - existing directories
/// - plain tokens (fallback [`Word`]
///
/// Serialized to Lua as a tagged table (`{ kind = "...", value = "..." }`).
#[derive(Serialize)]
#[serde(tag = "kind", content = "value")]
pub enum WordUnderCursor {
    /// A string that successfully parsed as a [`Url`] via [`Url::parse`].
    Url(String),
    /// A filesystem path identified as a binary file by [`exec_file_cmd`].
    BinaryFile(String),
    /// A filesystem path identified as a (plain / csv) text file by [`exec_file_cmd`].
    TextFile(String),
    /// A filesystem path identified as a directory by [`exec_file_cmd`].
    Directory(String),
    /// A fallback plain token (word) when no more specific classification applied.
    Word(String),
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
/// 2. Otherwise invokes [`exec_file_cmd`] to check filesystem type.
/// 3. Falls back to [`WordUnderCursor::Word`] on errors or unknown kinds.
impl From<String> for WordUnderCursor {
    fn from(value: String) -> Self {
        if Url::parse(&value).is_ok() {
            return Self::Url(value);
        }
        match exec_file_cmd(&value) {
            Ok(FileCmdOutput::BinaryFile(x)) => Self::BinaryFile(x),
            Ok(FileCmdOutput::TextFile(x)) => Self::TextFile(x),
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
///
/// Returns an error if:
/// - launching or waiting on the `file` command fails
/// - the command exits with non-success (see [`utils::cmd::CmdExt`])
/// - stdout cannot be decoded as valid UTF-8
fn exec_file_cmd(path: &str) -> color_eyre::Result<FileCmdOutput> {
    let output = std::str::from_utf8(&Command::new("file").args([path, "-I"]).exec()?.stdout)?.to_lowercase();
    if output.contains(" inode/directory;") {
        return Ok(FileCmdOutput::Directory(path.to_owned()));
    }
    if output.contains(" text/plain;") || output.contains(" text/csv;") {
        return Ok(FileCmdOutput::TextFile(path.to_owned()));
    }
    if output.contains(" text/binary;") {
        return Ok(FileCmdOutput::BinaryFile(path.to_owned()));
    }
    if output.contains(" no such file or directory") {
        return Ok(FileCmdOutput::NotFound(path.to_owned()));
    }
    Ok(FileCmdOutput::Unknown(path.to_owned()))
}

/// Raw classification result returned by [`exec_file_cmd`].
#[derive(Serialize)]
pub enum FileCmdOutput {
    BinaryFile(String),
    TextFile(String),
    Directory(String),
    NotFound(String),
    Unknown(String),
}

/// Finds the non-whitespace "word" in the supplied [`str`] containing the supplied visual index `idx`.
///
/// Returns [`Option::None`] if `idx`:
///
/// - is out of bounds
/// - doesn't point to a char boundary
/// - points to a whitespace
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

/// Converts a visual (character) index into a byte index for the supplied [`str`].
///
/// - Returns [`Option::Some`] [`usize`] for valid positions, including `s.len()` for end-of-line.
/// - Returns [`Option::None`] if `idx` is past the end (out of bounds).
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
    use super::*;

    #[test]
    fn get_word_at_index_returns_word_inside_ascii_word() {
        let s = "open file.txt now";
        let idx = 7;
        assert_eq!(Some("file.txt"), get_word_at_index(s, idx));
    }

    #[test]
    fn get_word_at_index_returns_word_at_start_and_end_boundaries() {
        let s = "yes run main.rs";
        let idx_start = 8;
        let idx_last_inside = 14;
        assert_eq!(Some("main.rs"), get_word_at_index(s, idx_start));
        assert_eq!(Some("main.rs"), get_word_at_index(s, idx_last_inside));
    }

    #[test]
    fn get_word_at_index_returns_none_on_whitespace() {
        let s = "hello  world";
        assert_eq!(None, get_word_at_index(s, 5));
        assert_eq!(None, get_word_at_index(s, 6));
    }

    #[test]
    fn get_word_at_index_includes_punctuation_in_word() {
        let s = "print(arg)";
        let idx = 5;
        assert_eq!(Some("print(arg)"), get_word_at_index(s, idx));
    }

    #[test]
    fn get_word_at_index_returns_word_at_line_boundaries() {
        let s = "/usr/local/bin";
        assert_eq!(Some("/usr/local/bin"), get_word_at_index(s, 0));
        assert_eq!(Some("/usr/local/bin"), get_word_at_index(s, 14));
    }

    #[test]
    fn get_word_at_index_handles_utf8_boundaries_and_space() {
        let s = "αβ γ";
        assert_eq!(Some("αβ"), get_word_at_index(s, 0));
        assert_eq!(Some("αβ"), get_word_at_index(s, 1));
        assert_eq!(Some("γ"), get_word_at_index(s, 4));
        assert_eq!(None, get_word_at_index(s, 5));
    }

    #[test]
    fn get_word_at_index_returns_none_with_index_out_of_bounds() {
        let s = "abc";
        assert_eq!(None, get_word_at_index(s, 10));
    }
}
