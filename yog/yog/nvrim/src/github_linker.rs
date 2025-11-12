//! GitHub permalink generation for selected code.
//!
//! Exposes a dictionary with a `get_link` function that constructs GitHub URLs for visually selected
//! code ranges in the current buffer, using the repository's current commit hash for permalinks.
//! The generated URL is automatically copied to the system clipboard.

use std::path::Path;

use nvim_oxi::Dictionary;
use ytil_github::RepoViewField;
use ytil_nvim_oxi::visual_selection::Bound;
use ytil_nvim_oxi::visual_selection::Selection;

/// [`Dictionary`] with GitHub link generation helpers.
pub fn dict() -> Dictionary {
    dict! {
        "get_link": fn_from!(get_link),
    }
}

/// Generates a GitHub permalink for the current visual selection and copies it to the clipboard.
///
/// # Arguments
/// - `link_type` The type of GitHub link to generate (e.g., "blob" for file view).
#[allow(clippy::needless_pass_by_value)]
fn get_link(link_type: String) {
    let Some(cur_buf_path) = ytil_nvim_oxi::buffer::get_relative_buffer_path(&nvim_oxi::api::get_current_buf()) else {
        return;
    };
    if cur_buf_path.as_os_str().is_empty() {
        return;
    }

    let Some(selection) = ytil_nvim_oxi::visual_selection::get(()) else {
        return;
    };
    let Ok(mut repo_url) = ytil_github::get_repo_view_field(&RepoViewField::Url).inspect_err(|error| {
        ytil_nvim_oxi::api::notify_error(format!("cannot get GitHub repo URL | error={error:#?}"));
    }) else {
        return;
    };

    let Ok(current_commit_hash) = ytil_git::get_current_commit_hash().inspect_err(|error| {
        ytil_nvim_oxi::api::notify_error(format!("cannot get current repo commit hash | error={error:#?}"));
    }) else {
        return;
    };

    build_github_file_url(
        &mut repo_url,
        &link_type,
        &current_commit_hash,
        &cur_buf_path,
        &selection,
    );

    cp_to_system_clipboard_and_notify_error(&mut repo_url.as_bytes());
}

/// Builds a GitHub file URL by appending link type, commit hash, file path, and selection range.
///
/// # Arguments
/// - `repo_url` The base repository URL to modify in-place.
/// - `link_type` The GitHub link type (e.g., "blob").
/// - `commit_hash` The commit hash for the permalink.
/// - `cur_buf_path` The relative path of the current buffer.
/// - `selection` The visual selection bounds.
///
/// # Rationale
/// `repo_url` is [`String`] instead of [`url::Url`] because working with [`url::Url`] is really painful.
/// `link_type` is [`&str`] instead of an enum because the [`&str`] is what will be used to build different links.
fn build_github_file_url(
    repo_url: &mut String,
    link_type: &str,
    commit_hash: &str,
    cur_buf_path: &Path,
    selection: &Selection,
) {
    repo_url.push('/');
    repo_url.push_str(link_type);
    repo_url.push('/');
    repo_url.push_str(commit_hash);
    repo_url.push('/');
    repo_url.push_str(cur_buf_path.to_string_lossy().trim_start_matches('/'));
    repo_url.push_str("?plain=1");
    repo_url.push('#');
    add_github_line_col_to_url(repo_url, selection.start());
    repo_url.push('-');
    add_github_line_col_to_url(repo_url, selection.end());
}

/// Appends a GitHub-style line and column anchor (e.g., L42C7) to the supplied URL.
///
/// # Arguments
/// - `repo_url` The URL string to append to.
/// - `bound` The line and column bound (1-based line, 0-based column).
fn add_github_line_col_to_url(repo_url: &mut String, bound: &Bound) {
    repo_url.push('L');
    repo_url.push_str(&bound.lnum.saturating_add(1).to_string());
    repo_url.push('C');
    repo_url.push_str(&bound.col.to_string());
}

/// Copies the given content to the system clipboard, notifying Nvim on error.
///
/// # Arguments
/// - `content` The bytes to copy to the clipboard.
fn cp_to_system_clipboard_and_notify_error(content: &mut &[u8]) {
    if let Err(error) = ytil_system::cp_to_system_clipboard(content) {
        ytil_nvim_oxi::api::notify_error(format!(
            "cannot copy content to system clipboard | content={content:?} error={error:#?}"
        ));
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::single_line_selection(
        "https://github.com/user/repo",
        "blob",
        "abc123",
        "/src/main.rs",
        Bound { lnum: 10, col: 5 },
        Bound { lnum: 10, col: 10 },
        "https://github.com/user/repo/blob/abc123/src/main.rs?plain=1#L11C5-L11C10"
    )]
    #[case::multi_line_selection(
        "https://github.com/user/repo",
        "blob",
        "def456",
        "/lib/utils.rs",
        Bound { lnum: 1, col: 0 },
        Bound { lnum: 3, col: 20 },
        "https://github.com/user/repo/blob/def456/lib/utils.rs?plain=1#L2C0-L4C20"
    )]
    #[case::root_file(
        "https://github.com/user/repo",
        "tree",
        "ghi789",
        "/README.md",
        Bound { lnum: 5, col: 2 },
        Bound { lnum: 5, col: 2 },
        "https://github.com/user/repo/tree/ghi789/README.md?plain=1#L6C2-L6C2"
    )]
    fn build_github_file_link_appends_correct_url(
        #[case] initial_repo_url: &str,
        #[case] url_kind: &str,
        #[case] commit_hash: &str,
        #[case] file_path: &str,
        #[case] start: Bound,
        #[case] end: Bound,
        #[case] expected: &str,
    ) {
        let mut repo_url = initial_repo_url.to_string();
        let cur_buf_path = Path::new(file_path);
        let selection = {
            use ytil_nvim_oxi::visual_selection::SelectionBounds;

            let bounds = SelectionBounds { buf_id: 1, start, end };
            Selection::new(bounds, std::iter::empty::<nvim_oxi::String>())
        };

        build_github_file_url(&mut repo_url, url_kind, commit_hash, cur_buf_path, &selection);

        pretty_assertions::assert_eq!(repo_url, expected);
    }
}
