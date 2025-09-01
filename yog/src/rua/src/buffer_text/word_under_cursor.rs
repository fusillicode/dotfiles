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
///
/// # Examples
/// ```
/// let s = "open file.txt now";
/// assert_eq!(Some("file.txt"), get_word_at_index(s, 7));
/// assert_eq!(None, get_word_at_index(s, 5)); // space
/// let s = "αβ γ";
/// assert_eq!(Some("αβ"), get_word_at_index(s, 0));
/// assert_eq!(None, get_word_at_index(s, 1)); // not a boundary
/// assert_eq!(Some("γ"), get_word_at_index(s, 5));
/// ```
fn get_word_at_index(s: &str, idx: usize) -> Option<&str> {
    if idx > s.len() || (idx < s.len() && !s.is_char_boundary(idx)) {
        return None;
    }
    let is_space = |c: char| c.is_whitespace();

    let in_word = if idx < s.len() {
        s[idx..].chars().next().is_some_and(|c| !is_space(c))
    } else {
        s.chars().next_back().is_some_and(|c| !is_space(c))
    };
    if !in_word {
        return None;
    }

    let mut left = idx;
    while left > 0 {
        let Some((p, ch)) = s[..left].char_indices().next_back() else {
            break;
        };
        if is_space(ch) {
            break;
        }
        left = p;
    }

    let mut right = idx;
    while right < s.len() {
        if let Some((offset, ch)) = s[right..].char_indices().next() {
            if is_space(ch) {
                break;
            }
            right = right.saturating_add(offset).saturating_add(ch.len_utf8());
        } else {
            break;
        }
    }

    Some(&s[left..right])
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
        assert_eq!(None, get_word_at_index(s, 1));
        assert_eq!(None, get_word_at_index(s, 4));
        assert_eq!(Some("γ"), get_word_at_index(s, 5));
    }

    #[test]
    fn get_word_at_index_returns_none_with_index_out_of_bounds() {
        let s = "abc";
        assert_eq!(None, get_word_at_index(s, 10));
    }
}
