use nvim_oxi::api::Window;

/// Gets the non-whitespace "word" under the cursor in the current window.
/// On failure returns [`Option::None`] and notifies an error to Neovim.
/// If on a whitespace returns an [`Option::None`].
pub fn get(_: ()) -> Option<String> {
    let cur_win = Window::current();
    let cur_line = nvim_oxi::api::get_current_line()
        .inspect_err(|e| crate::oxi_ext::notify_error(&format!("cannot get current line: {e:#?}")))
        .ok()?;
    let (_, col) = cur_win
        .get_cursor()
        .inspect_err(|e| crate::oxi_ext::notify_error(&format!("cannot get cursor: {e:#?}")))
        .ok()?;
    get_word_at_index(&cur_line, col).map(ToOwned::to_owned)
}

/// Finds the non-whitespace "word" in `s` containing byte index `idx`.
///
/// Returns [`Option::None`] if:
///
/// - `idx` is out of bounds
/// - not on a char boundary
/// - not on a whitespace
fn get_word_at_index(s: &str, idx: usize) -> Option<&str> {
    // Convert visual (char) index to byte index; allow end-of-line.
    let mut chars_seen = 0usize;
    let mut byte_idx = None;
    for (b, _) in s.char_indices() {
        if chars_seen == idx {
            byte_idx = Some(b);
            break;
        }
        chars_seen += 1;
    }
    let idx = match byte_idx {
        Some(b) => b,
        None if idx == chars_seen => s.len(), // end-of-line
        _ => return None,                     // out of bounds
    };

    // If pointing to whitespace, no word.
    if s[idx..].chars().next().is_some_and(char::is_whitespace) {
        return None;
    }

    // Scan split words and see which span contains idx.
    let mut pos = 0;
    for word in s.split_ascii_whitespace() {
        let start = s[pos..].find(word)? + pos;
        let end = start + word.len();
        if (start..=end).contains(&idx) {
            return Some(word);
        }
        pos = end;
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
