//! GitHub permalink generation for selected code.
//!
//! Exposes a dictionary with a `get_link` function that constructs GitHub URLs for visually selected
//! code ranges in the current buffer, using the repository's current commit hash for permalinks.
//! The generated URL is automatically copied to the system clipboard.

use std::path::Path;

use nvim_oxi::Dictionary;
use ytil_git::remote::GitProvider;
use ytil_noxi::visual_selection::Bound;
use ytil_noxi::visual_selection::Selection;

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
fn get_link((link_type, open): (String, Option<bool>)) {
    let Some(current_buffer_path) = ytil_noxi::buffer::get_relative_path_to_cwd(&nvim_oxi::api::get_current_buf())
    else {
        return;
    };
    if current_buffer_path.as_os_str().is_empty() {
        return;
    }

    let Some(selection) = ytil_noxi::visual_selection::get(()) else {
        return;
    };

    let Ok(repo) = ytil_git::discover_repo(Path::new(".")).inspect_err(|err| {
        ytil_noxi::notify::error(err);
    }) else {
        return;
    };

    let Ok(repo_urls) = ytil_git::remote::get_https_urls(&repo).inspect_err(|err| {
        ytil_noxi::notify::error(format!("error discovering git repo | error={err:#?}"));
    }) else {
        return;
    };

    // FIXME: handle case of multiple remotes
    let Some(mut repo_url) = repo_urls.into_iter().next() else {
        return;
    };

    let Ok(Some(git_provider)) = GitProvider::get(&repo_url)
        .inspect_err(|err| {
            ytil_noxi::notify::error(format!(
                "error getting git provider for url | url={repo_url:#?} error={err:?}"
            ));
        })
        .inspect(|gp| {
            if gp.is_none() {
                ytil_noxi::notify::error(format!("error no git provider found for url | url={repo_url:#?}"));
            }
        })
    else {
        return;
    };

    let Ok(current_commit_hash) = ytil_git::get_current_commit_hash(&repo).inspect_err(|err| {
        ytil_noxi::notify::error(format!("error getting current repo commit hash | error={err:#?}"));
    }) else {
        return;
    };

    build_file_url(
        &mut repo_url,
        &git_provider,
        &link_type,
        &current_commit_hash,
        &current_buffer_path,
        &selection,
    );

    if open.is_some_and(std::convert::identity) {
        if let Err(err) = ytil_sys::open(&repo_url) {
            ytil_noxi::notify::error(format!("error opening file URL | repo_url={repo_url:?} error={err:#?}"));
        }
    } else {
        if let Err(err) = ytil_sys::file::cp_to_system_clipboard(&mut repo_url.as_bytes()) {
            ytil_noxi::notify::error(format!(
                "error copying content to system clipboard | content={repo_url:?} error={err:#?}"
            ));
        }
        nvim_oxi::print!("URL copied to clipboard:\n{repo_url}");
    }
}

fn build_file_url(
    repo_url: &mut String,
    git_provider: &GitProvider,
    link_type: &str,
    commit_hash: &str,
    current_buffer_path: &Path,
    selection: &Selection,
) {
    repo_url.push('/');
    repo_url.push_str(link_type);
    repo_url.push('/');
    repo_url.push_str(commit_hash);
    repo_url.push('/');
    repo_url.push_str(current_buffer_path.to_string_lossy().trim_start_matches('/'));

    match git_provider {
        GitProvider::GitHub => append_github_file_selection(repo_url, selection),
        GitProvider::GitLab => append_gitlab_file_selection(repo_url, selection),
    }
}

fn append_github_file_selection(repo_url: &mut String, selection: &Selection) {
    fn add_github_lnum_and_col_to_url(repo_url: &mut String, bound: &Bound) {
        repo_url.push('L');
        append_lnum(repo_url, bound);
        repo_url.push('C');
        repo_url.push_str(&bound.col.to_string());
    }
    repo_url.push_str("?plain=1");
    repo_url.push('#');
    add_github_lnum_and_col_to_url(repo_url, selection.start());
    repo_url.push('-');
    add_github_lnum_and_col_to_url(repo_url, selection.end());
}

fn append_lnum(repo_url: &mut String, bound: &Bound) {
    repo_url.push_str(&bound.lnum.saturating_add(1).to_string());
}

fn append_gitlab_file_selection(repo_url: &mut String, selection: &Selection) {
    repo_url.push('#');
    repo_url.push('L');
    append_lnum(repo_url, selection.start());
    repo_url.push('-');
    append_lnum(repo_url, selection.end());
}

#[cfg(test)]
mod tests {
    // At module level because over rstest function it doesn't work.
    #![allow(clippy::too_many_arguments)]

    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::github_single_line_selection(
        "https://github.com/user/repo",
        GitProvider::GitHub,
        "blob",
        "abc123",
        "/src/main.rs",
        Bound { lnum: 10, col: 5 },
        Bound { lnum: 10, col: 10 },
        "https://github.com/user/repo/blob/abc123/src/main.rs?plain=1#L11C5-L11C10"
    )]
    #[case::github_multi_line_selection(
        "https://github.com/user/repo",
        GitProvider::GitHub,
        "blob",
        "def456",
        "/lib/utils.rs",
        Bound { lnum: 1, col: 0 },
        Bound { lnum: 3, col: 20 },
        "https://github.com/user/repo/blob/def456/lib/utils.rs?plain=1#L2C0-L4C20"
    )]
    #[case::github_root_file(
        "https://github.com/user/repo",
        GitProvider::GitHub,
        "tree",
        "ghi789",
        "/README.md",
        Bound { lnum: 5, col: 2 },
        Bound { lnum: 5, col: 2 },
        "https://github.com/user/repo/tree/ghi789/README.md?plain=1#L6C2-L6C2"
    )]
    #[case::gitlab_single_line_selection(
        "https://gitlab.com/user/repo",
        GitProvider::GitLab,
        "blob",
        "abc123",
        "/src/main.rs",
        Bound { lnum: 10, col: 5 },
        Bound { lnum: 10, col: 10 },
        "https://gitlab.com/user/repo/blob/abc123/src/main.rs#L11-11"
    )]
    #[case::gitlab_multi_line_selection(
        "https://gitlab.com/user/repo",
        GitProvider::GitLab,
        "blob",
        "def456",
        "/lib/utils.rs",
        Bound { lnum: 1, col: 0 },
        Bound { lnum: 3, col: 20 },
        "https://gitlab.com/user/repo/blob/def456/lib/utils.rs#L2-4"
    )]
    #[case::gitlab_root_file(
        "https://gitlab.com/user/repo",
        GitProvider::GitLab,
        "tree",
        "ghi789",
        "/README.md",
        Bound { lnum: 5, col: 2 },
        Bound { lnum: 5, col: 2 },
        "https://gitlab.com/user/repo/tree/ghi789/README.md#L6-6"
    )]
    fn build_file_url_works_as_expected(
        #[case] initial_repo_url: &str,
        #[case] git_provider: GitProvider,
        #[case] url_kind: &str,
        #[case] commit_hash: &str,
        #[case] file_path: &str,
        #[case] start: Bound,
        #[case] end: Bound,
        #[case] expected: &str,
    ) {
        let mut repo_url = initial_repo_url.to_string();
        let current_buffer_path = Path::new(file_path);
        let selection = {
            use ytil_noxi::visual_selection::SelectionBounds;

            let bounds = SelectionBounds { buf_id: 1, start, end };
            Selection::new(bounds, std::iter::empty::<nvim_oxi::String>())
        };

        build_file_url(
            &mut repo_url,
            &git_provider,
            url_kind,
            commit_hash,
            current_buffer_path,
            &selection,
        );

        pretty_assertions::assert_eq!(repo_url, expected);
    }
}
