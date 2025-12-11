//! Token classification under cursor (URL / file / directory / word).
//!
//! Retrieves current line + cursor column, extracts contiguous non‑whitespace token, classifies via
//! filesystem inspection or URL parsing, returning a tagged Lua table.

use color_eyre::eyre::Context;
use color_eyre::eyre::bail;
use nvim_oxi::Object;
use nvim_oxi::api::Buffer;
use nvim_oxi::api::Window;
use nvim_oxi::conversion::ToObject;
use nvim_oxi::lua::ffi::State;
use nvim_oxi::serde::Serializer;
use serde::Serialize;
use url::Url;
use ytil_noxi::buffer::BufferExt;
use ytil_noxi::buffer::CursorPosition;
use ytil_sys::file::FileCmdOutput;
use ytil_sys::lsof::ProcessFilter;

/// Retrieve and classify the non-whitespace token under the cursor in the current window.
///
/// Returns [`Option::None`] if the current line or cursor position cannot be obtained,
/// or if the cursor is on whitespace. On errors a notification is emitted to Nvim.
/// On success returns a classified [`TokenUnderCursor`].
pub fn get(_: ()) -> Option<TokenUnderCursor> {
    let current_buffer = nvim_oxi::api::get_current_buf();
    let cursor_pos = CursorPosition::get_current()?;

    let token_under_cursor = if current_buffer.is_terminal() {
        get_token_under_cursor_in_terminal_buffer(&current_buffer, &cursor_pos)
    } else {
        get_token_under_cursor_in_normal_buffer(&cursor_pos)
    }
    .as_deref()
    .map(TokenUnderCursor::classify)?
    .inspect_err(|err| ytil_noxi::notify::error(format!("error classifying word under cursor | error={err:?}")))
    .ok()?;

    let token_under_cursor = token_under_cursor
        .refine_word(&current_buffer)
        .inspect_err(|err| ytil_noxi::notify::error(format!("error refining word under cursor | error={err:?}")))
        .ok()?;

    Some(token_under_cursor)
}

fn get_token_under_cursor_in_terminal_buffer(buffer: &Buffer, cursor_pos: &CursorPosition) -> Option<String> {
    let window_width = Window::current()
        .get_width()
        .wrap_err("error getting window width")
        .and_then(|x| {
            usize::try_from(x).wrap_err_with(|| format!("error converting window width to usize | width={x}"))
        })
        .inspect_err(|err| ytil_noxi::notify::error(format!("{err}")))
        .ok()?
        .saturating_sub(1);

    let mut out = vec![];
    let mut word_end_idx = 0;
    for (idx, current_char) in ytil_noxi::buffer::get_current_line()?.char_indices() {
        word_end_idx = idx;
        if idx < cursor_pos.col {
            if current_char.is_ascii_whitespace() {
                out.clear();
            } else {
                out.push(current_char);
            }
        } else if idx > cursor_pos.col {
            if current_char.is_ascii_whitespace() {
                break;
            }
            out.push(current_char);
        } else if current_char.is_ascii_whitespace() {
            out.clear();
            out.push(current_char);
            break;
        } else {
            out.push(current_char);
        }
    }

    // Check rows before the cursor one.
    if word_end_idx.saturating_sub(out.len()) == 0 {
        'outer: for idx in (0..cursor_pos.row.saturating_sub(1)).rev() {
            let line = buffer.get_line(idx).ok()?.to_string_lossy().to_string();
            if line.is_empty() {
                break 'outer;
            }
            if let Some((_, prev)) = line.rsplit_once(' ') {
                out.splice(0..0, prev.chars());
                break;
            }
            if line.chars().count() < window_width {
                break;
            }
            out.splice(0..0, line.chars());
        }
    }

    // Check rows after the cursor one.
    if word_end_idx >= window_width {
        'outer: for idx in cursor_pos.row..usize::MAX {
            let line = buffer.get_line(idx).ok()?.to_string_lossy().to_string();
            if line.is_empty() {
                break 'outer;
            }
            if let Some((next, _)) = line.split_once(' ') {
                out.extend(next.chars());
                break;
            }
            out.extend(line.chars());
            if line.chars().count() < window_width {
                break;
            }
        }
    }

    Some(out.into_iter().collect())
}

fn get_token_under_cursor_in_normal_buffer(cursor_pos: &CursorPosition) -> Option<String> {
    let current_line = ytil_noxi::buffer::get_current_line()?;
    get_word_at_index(&current_line, cursor_pos.col).map(ToOwned::to_owned)
}

/// Classified representation of the token found under the cursor.
///
/// Used to distinguish between:
/// - URLs
/// - existing binary files
/// - existing text files
/// - existing directories
/// - plain tokens (fallback [`TokenUnderCursor::MaybeTextFile`])
///
/// Serialized to Lua as a tagged table (`{ kind = "...", value = "..." }`).
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", content = "value")]
#[cfg_attr(test, derive(Eq, PartialEq))]
pub enum TokenUnderCursor {
    /// A string that successfully parsed as a [`Url`] via [`Url::parse`].
    Url(String),
    /// A filesystem path identified as a binary file by [`ytil_sys::file::exec_file_cmd`].
    BinaryFile(String),
    /// A filesystem path identified as a text file by [`ytil_sys::file::exec_file_cmd`].
    TextFile {
        path: String,
        lnum: Option<i64>,
        col: Option<i64>,
    },
    /// A filesystem path identified as a directory by [`ytil_sys::file::exec_file_cmd`].
    Directory(String),
    /// A fallback plain token (word) when no more specific classification applied.
    MaybeTextFile {
        value: String,
        lnum: Option<i64>,
        col: Option<i64>,
    },
}

impl nvim_oxi::lua::Pushable for TokenUnderCursor {
    unsafe fn push(self, lstate: *mut State) -> Result<std::ffi::c_int, nvim_oxi::lua::Error> {
        unsafe {
            self.to_object()
                .map_err(nvim_oxi::lua::Error::push_error_from_err::<Self, _>)?
                .push(lstate)
        }
    }
}

impl ToObject for TokenUnderCursor {
    fn to_object(self) -> Result<Object, nvim_oxi::conversion::Error> {
        self.serialize(Serializer::new()).map_err(Into::into)
    }
}

/// Classify a [`String`] captured under the cursor into a [`TokenUnderCursor`].
///
/// 1. If it parses as a URL with [`Url::parse`], returns [`TokenUnderCursor::Url`].
/// 2. Otherwise, invokes [`ytil_sys::file::exec_file_cmd`] to check filesystem type.
/// 3. Falls back to [`TokenUnderCursor::MaybeTextFile`] on errors or unknown kinds.
impl TokenUnderCursor {
    fn classify(value: &str) -> color_eyre::Result<Self> {
        let value = value.trim_matches('"').trim_matches('`').trim_matches('\'').to_string();

        if Url::parse(&value).is_ok() {
            return Ok(Self::Url(value));
        }

        Self::classify_not_url(value)
    }

    fn classify_not_url(value: String) -> color_eyre::Result<Self> {
        let mut parts = value.split(':');

        let Some(maybe_path) = parts.next() else {
            return Ok(Self::MaybeTextFile {
                value,
                lnum: None,
                col: None,
            });
        };

        let lnum = parts.next().map(str::parse).transpose().ok().flatten();
        let col = parts.next().map(str::parse).transpose().ok().flatten();

        Ok(match ytil_sys::file::exec_file_cmd(maybe_path)? {
            FileCmdOutput::BinaryFile(x) => Self::BinaryFile(x),
            FileCmdOutput::TextFile(path) => Self::TextFile { path, lnum, col },
            FileCmdOutput::Directory(x) => Self::Directory(x),
            FileCmdOutput::NotFound(path) | FileCmdOutput::Unknown(path) => {
                Self::MaybeTextFile { value: path, lnum, col }
            }
        })
    }

    fn refine_word(&self, buffer: &Buffer) -> color_eyre::Result<Self> {
        if let Self::MaybeTextFile { value, lnum, col } = self {
            let pid = buffer.get_pid()?;

            let mut lsof_res = ytil_sys::lsof::lsof(&ProcessFilter::Pid(&pid))?;

            let Some(process_desc) = lsof_res.get_mut(0) else {
                bail!("error no process found for pid | pid={pid:?}");
            };

            let maybe_path = {
                process_desc.cwd.push(value);
                let mut tmp = process_desc.cwd.to_string_lossy().to_string();
                if let Some(lnum) = lnum {
                    tmp.push(':');
                    tmp.push_str(&lnum.to_string());
                }
                if let Some(col) = col {
                    tmp.push(':');
                    tmp.push_str(&col.to_string());
                }
                tmp
            };

            return Self::classify_not_url(maybe_path);
        }
        Ok(self.clone())
    }
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
    #[cfg(target_os = "macos")]
    use tempfile::NamedTempFile;
    #[cfg(target_os = "macos")]
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

    // Tests are skipped in CI because [`TokenUnderCursor::from`] calls `file` command and that
    // behaves differently based on the platform (e.g. macOS vs Linux)

    #[test]
    #[cfg(target_os = "macos")]
    fn token_under_cursor_classify_valid_url_returns_url() {
        let input = "https://example.com".to_string();
        let result = TokenUnderCursor::classify(&input);
        assert2::let_assert!(Ok(actual) = result);
        pretty_assertions::assert_eq!(actual, TokenUnderCursor::Url(input));
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn token_under_cursor_classify_invalid_url_plain_word_returns_word() {
        let input = "noturl".to_string();
        let result = TokenUnderCursor::classify(&input);
        assert2::let_assert!(Ok(actual) = result);
        pretty_assertions::assert_eq!(
            actual,
            TokenUnderCursor::MaybeTextFile {
                value: input,
                lnum: None,
                col: None
            }
        );
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn token_under_cursor_classify_path_to_text_file_returns_text_file() {
        let mut temp_file = NamedTempFile::new().unwrap();
        std::io::Write::write_all(&mut temp_file, b"hello world").unwrap();
        let path = temp_file.path().to_string_lossy().to_string();
        let result = TokenUnderCursor::classify(&path);
        assert2::let_assert!(Ok(actual) = result);
        pretty_assertions::assert_eq!(
            actual,
            TokenUnderCursor::TextFile {
                path,
                lnum: None,
                col: None
            }
        );
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn token_under_cursor_classify_path_lnum_to_text_file_returns_text_file_with_lnum() {
        let mut temp_file = NamedTempFile::new().unwrap();
        std::io::Write::write_all(&mut temp_file, b"hello world").unwrap();
        let path = temp_file.path().to_string_lossy().to_string();
        let result = TokenUnderCursor::classify(&format!("{path}:10"));
        assert2::let_assert!(Ok(actual) = result);
        pretty_assertions::assert_eq!(
            actual,
            TokenUnderCursor::TextFile {
                path,
                lnum: Some(10),
                col: None
            }
        );
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn token_under_cursor_classify_path_lnum_col_to_text_file_returns_text_file_with_lnum_col() {
        let mut temp_file = NamedTempFile::new().unwrap();
        std::io::Write::write_all(&mut temp_file, b"hello world").unwrap();
        let path = temp_file.path().to_string_lossy().to_string();
        let result = TokenUnderCursor::classify(&format!("{path}:10:5"));
        assert2::let_assert!(Ok(actual) = result);
        pretty_assertions::assert_eq!(
            actual,
            TokenUnderCursor::TextFile {
                path,
                lnum: Some(10),
                col: Some(5)
            }
        );
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn token_under_cursor_classify_path_to_directory_returns_directory() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().to_string_lossy().to_string();
        let result = TokenUnderCursor::classify(&path);
        assert2::let_assert!(Ok(actual) = result);
        pretty_assertions::assert_eq!(actual, TokenUnderCursor::Directory(path));
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn token_under_cursor_classify_path_to_binary_file_returns_binary_file() {
        let mut temp_file = NamedTempFile::new().unwrap();
        // Write some binary data
        std::io::Write::write_all(&mut temp_file, &[0, 1, 2, 255]).unwrap();
        let path = temp_file.path().to_string_lossy().to_string();
        let result = TokenUnderCursor::classify(&path);
        assert2::let_assert!(Ok(actual) = result);
        pretty_assertions::assert_eq!(actual, TokenUnderCursor::BinaryFile(path));
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn token_under_cursor_classify_nonexistent_path_returns_maybe_text_file() {
        let path = "/nonexistent/path".to_string();
        let result = TokenUnderCursor::classify(&path);
        assert2::let_assert!(Ok(actual) = result);
        pretty_assertions::assert_eq!(
            actual,
            TokenUnderCursor::MaybeTextFile {
                value: path,
                lnum: None,
                col: None
            }
        );
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn token_under_cursor_classify_path_with_invalid_lnum_returns_maybe_text_file() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_string_lossy().to_string();
        let input = format!("{path}:invalid");
        let result = TokenUnderCursor::classify(&input);
        assert2::let_assert!(Ok(actual) = result);
        pretty_assertions::assert_eq!(
            actual,
            TokenUnderCursor::MaybeTextFile {
                value: path,
                lnum: None,
                col: None
            }
        );
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn token_under_cursor_classify_path_with_invalid_col_returns_maybe_text_file() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_string_lossy().to_string();
        let input = format!("{path}:10:invalid");
        let result = TokenUnderCursor::classify(&input);
        assert2::let_assert!(Ok(actual) = result);
        pretty_assertions::assert_eq!(
            actual,
            TokenUnderCursor::MaybeTextFile {
                value: path,
                lnum: Some(10),
                col: None
            }
        );
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn token_under_cursor_classify_path_lnum_col_extra_ignores_extra() {
        let mut temp_file = NamedTempFile::new().unwrap();
        std::io::Write::write_all(&mut temp_file, b"hello world").unwrap();
        let path = temp_file.path().to_string_lossy().to_string();
        let result = TokenUnderCursor::classify(&format!("{path}:10:5:extra"));
        assert2::let_assert!(Ok(actual) = result);
        pretty_assertions::assert_eq!(
            actual,
            TokenUnderCursor::TextFile {
                path,
                lnum: Some(10),
                col: Some(5)
            }
        );
    }
}
