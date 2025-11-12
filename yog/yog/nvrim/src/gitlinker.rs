use std::path::Path;

use nvim_oxi::Dictionary;
use ytil_nvim_oxi::visual_selection::Bound;
use ytil_nvim_oxi::visual_selection::Selection;

pub fn dict() -> Dictionary {
    dict! {
        "get_link": fn_from!(get_link),
        "get_blame_link": fn_from!(get_blame_link),
    }
}

fn get_link(_: Option<()>) {
    let Some(cur_buf_path) = ytil_nvim_oxi::buffer::get_relative_buffer_path(&nvim_oxi::api::get_current_buf()) else {
        return;
    };
    if cur_buf_path.as_os_str().is_empty() {
        return;
    }

    let Some(selection) = ytil_nvim_oxi::visual_selection::get(()) else {
        return;
    };
    let Ok(mut repo_url) = ytil_github::get_repo_view_field(ytil_github::RepoViewField::Url).inspect_err(|error| {
        ytil_nvim_oxi::api::notify_error(format!("cannot get GitHub repo URL | error={error:#?}"));
    }) else {
        return;
    };

    let Ok(current_commit_hash) = ytil_git::get_current_commit_hash().inspect_err(|error| {
        ytil_nvim_oxi::api::notify_error(format!("cannot get current repo commit hash | error={error:#?}"));
    }) else {
        return;
    };

    build_github_file_link(&mut repo_url, "blob", &current_commit_hash, &cur_buf_path, selection);

    cp_to_system_clipboard_and_notify_error(&mut repo_url.to_string().as_bytes());
}

fn get_blame_link(_: Option<()>) {
    let link_to_file = "foo";
    cp_to_system_clipboard_and_notify_error(&mut link_to_file.as_bytes());
}

/// Using a [`String`] instead of an [`Url`] because working with that is a PITA.
fn build_github_file_link(
    repo_url: &mut String,
    url_kind: &str,
    commit_hash: &str,
    cur_buf_path: &Path,
    selection: Selection,
) {
    repo_url.push('/');
    repo_url.push_str(url_kind);
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

fn add_github_line_col_to_url(repo_url: &mut String, bound: &Bound) {
    repo_url.push('L');
    repo_url.push_str(&bound.lnum.to_string());
    repo_url.push('C');
    repo_url.push_str(&bound.col.to_string());
}

fn cp_to_system_clipboard_and_notify_error(content: &mut &[u8]) {
    if let Err(error) = ytil_system::cp_to_system_clipboard(content) {
        ytil_nvim_oxi::api::notify_error(format!(
            "cannot copy content to system clipboard | content={content:?} error={error:#?}"
        ));
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use rstest::rstest;
    use ytil_nvim_oxi::visual_selection::Bound;
    use ytil_nvim_oxi::visual_selection::Selection;
    use ytil_nvim_oxi::visual_selection::SelectionBounds;

    use super::*;

    #[rstest]
    #[case::single_line_selection(
        "https://github.com/user/repo",
        "blob",
        "abc123",
        "/src/main.rs",
        Bound { lnum: 10, col: 5 },
        Bound { lnum: 10, col: 10 },
        "https://github.com/user/repo/blob/abc123/src/main.rs?plain=1#L10C5-L10C10"
    )]
    #[case::multi_line_selection(
        "https://github.com/user/repo",
        "blob",
        "def456",
        "/lib/utils.rs",
        Bound { lnum: 1, col: 0 },
        Bound { lnum: 3, col: 20 },
        "https://github.com/user/repo/blob/def456/lib/utils.rs?plain=1#L1C0-L3C20"
    )]
    #[case::root_file(
        "https://github.com/user/repo",
        "tree",
        "ghi789",
        "/README.md",
        Bound { lnum: 5, col: 2 },
        Bound { lnum: 5, col: 2 },
        "https://github.com/user/repo/tree/ghi789/README.md?plain=1#L5C2-L5C2"
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
            let bounds = SelectionBounds { buf_id: 1, start, end };
            Selection::new(bounds, std::iter::empty::<nvim_oxi::String>())
        };

        build_github_file_link(&mut repo_url, url_kind, commit_hash, cur_buf_path, selection);

        pretty_assertions::assert_eq!(repo_url, expected);
    }
}
