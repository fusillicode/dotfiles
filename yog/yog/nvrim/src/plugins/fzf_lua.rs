use nvim_oxi::Dictionary;

/// [`Dictionary`] of `fzf-lua` helpers for previewless pickers.
///
/// `fzf-lua`'s built-in `line_query` depends on previewer state, so `path:line`
/// can fail when preview is disabled.
pub fn dict() -> Dictionary {
    dict! {
        "get_fd_flags": fn_from!(crate::cli::get_fd_flags),
        "get_rg_flags": fn_from!(crate::cli::get_rg_flags),
        "parse_line_query": fn_from!(parse_line_query),
    }
}

/// Parses trailing `:<digits>` from live query or persisted fallback query.
fn parse_line_query((query, last_query): (Option<String>, Option<String>)) -> Option<(String, Option<String>)> {
    let query = query
        .filter(|x| !x.is_empty())
        .or_else(|| last_query.filter(|x| !x.is_empty()))?;

    let (new_query, lnum) = query.rsplit_once(':')?;

    if lnum.is_empty() || !lnum.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }

    Some((lnum.to_string(), (!new_query.is_empty()).then(|| new_query.to_string())))
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(
        Some("dir/foo/bar:42"),
        Some("dir/foo/bar:9"),
        Some(("42".to_string(), Some("dir/foo/bar".to_string())))
    )]
    #[case(
        None,
        Some("dir/foo/bar:42"),
        Some(("42".to_string(), Some("dir/foo/bar".to_string())))
    )]
    #[case(
        Some(":42"),
        None,
        Some(("42".to_string(), Option::<String>::None))
    )]
    #[case(Some("dir/foo/bar"), None, None)]
    #[case(Some("dir/foo/bar:abc"), None, None)]
    #[case(None, None, None)]
    fn parse_line_query_works(
        #[case] query: Option<&str>,
        #[case] last_query: Option<&str>,
        #[case] expected: Option<(String, Option<String>)>,
    ) {
        let actual = parse_line_query((query.map(ToOwned::to_owned), last_query.map(ToOwned::to_owned)));

        pretty_assertions::assert_eq!(actual, expected);
    }
}
