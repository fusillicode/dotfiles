//! GitHub permalink generation for selected code.
//!
//! Exposes a dictionary with a `get_link` function that constructs GitHub URLs for visually selected
//! code ranges in the current buffer, using the repository's current commit hash for permalinks.
//! The generated URL is automatically copied to the system clipboard.

use std::fmt::Write as _;
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
#[allow(clippy::needless_pass_by_value)]
fn get_link((link_type, open): (String, Option<bool>)) -> Option<()> {
    let selection = ytil_noxi::visual_selection::get(())?;

    let repo = ytil_git::repo::discover(Path::new("."))
        .inspect_err(|err| {
            ytil_noxi::notify::error(err);
        })
        .ok()?;

    let cur_buf = nvim_oxi::api::get_current_buf();
    let abs_buf_path = ytil_noxi::buffer::get_absolute_path(Some(&cur_buf))
        .ok_or_else(|| {
            ytil_noxi::notify::error(format!(
                "error getting absolute path for current_buffer | current_buffer={cur_buf:?}"
            ));
        })
        .ok()?;
    let current_buffer_path = ytil_git::repo::get_relative_path_to_repo(&abs_buf_path, &repo)
        .inspect_err(|err| ytil_noxi::notify::error(err))
        .ok()?;

    let repo_urls = ytil_git::remote::get_https_urls(&repo)
        .inspect_err(|err| {
            ytil_noxi::notify::error(format!("error discovering git repo | error={err:#?}"));
        })
        .ok()?;

    // FIXME: handle case of multiple remotes
    let mut repo_url = repo_urls.into_iter().next()?;

    let current_commit_hash = ytil_git::get_current_commit_hash(&repo)
        .inspect_err(|err| {
            ytil_noxi::notify::error(format!("error getting current repo commit hash | error={err:#?}"));
        })
        .ok()?;

    let git_provider = GitProvider::get(&repo_url)
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
        .ok()??;

    match git_provider {
        GitProvider::GitHub => build_github_file_url(
            &mut repo_url,
            &link_type,
            &current_commit_hash,
            &current_buffer_path,
            &selection,
        ),
        GitProvider::GitLab => build_gitlab_file_url(
            &mut repo_url,
            &link_type,
            &current_commit_hash,
            &current_buffer_path,
            &selection,
        ),
    }

    if open.is_some_and(std::convert::identity) {
        ytil_sys::open(&repo_url)
            .inspect_err(|err| {
                ytil_noxi::notify::error(format!("error opening file URL | repo_url={repo_url:?} error={err:#?}"));
            })
            .ok()?;
    } else {
        ytil_sys::file::cp_to_system_clipboard(&mut repo_url.as_bytes())
            .inspect_err(|err| {
                ytil_noxi::notify::error(format!(
                    "error copying content to system clipboard | content={repo_url:?} error={err:#?}"
                ));
            })
            .ok()?;
        nvim_oxi::print!("URL copied to clipboard:\n{repo_url}");
    }

    Some(())
}

fn build_github_file_url(
    repo_url: &mut String,
    link_type: &str,
    commit_hash: &str,
    current_buffer_path: &Path,
    selection: &Selection,
) {
    fn add_github_lnum_and_col_to_url(repo_url: &mut String, bound: &Bound) {
        repo_url.push('L');
        append_lnum(repo_url, bound);
        repo_url.push('C');
        // write! to String is infallible; avoids intermediate String allocation from .to_string()
        let _ = write!(repo_url, "{}", bound.col);
    }

    repo_url.push('/');
    repo_url.push_str(link_type);
    repo_url.push('/');
    repo_url.push_str(commit_hash);
    repo_url.push('/');
    repo_url.push_str(current_buffer_path.to_string_lossy().trim_start_matches('/'));

    repo_url.push_str("?plain=1");
    repo_url.push('#');
    add_github_lnum_and_col_to_url(repo_url, selection.start());
    repo_url.push('-');
    add_github_lnum_and_col_to_url(repo_url, selection.end());
}

fn build_gitlab_file_url(
    repo_url: &mut String,
    link_type: &str,
    commit_hash: &str,
    current_buffer_path: &Path,
    selection: &Selection,
) {
    repo_url.push('/');
    repo_url.push('-');

    repo_url.push('/');
    repo_url.push_str(link_type);
    repo_url.push('/');
    repo_url.push_str(commit_hash);
    repo_url.push('/');
    repo_url.push_str(current_buffer_path.to_string_lossy().trim_start_matches('/'));

    repo_url.push('#');
    repo_url.push('L');
    append_lnum(repo_url, selection.start());
    repo_url.push('-');
    append_lnum(repo_url, selection.end());
}

fn append_lnum(repo_url: &mut String, bound: &Bound) {
    // write! to String is infallible; avoids intermediate String allocation from .to_string()
    let _ = write!(repo_url, "{}", bound.lnum.saturating_add(1));
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
    fn build_github_file_url_works_as_expected(
        #[case] initial_repo_url: &str,
        #[case] url_kind: &str,
        #[case] commit_hash: &str,
        #[case] file_path: &str,
        #[case] start: Bound,
        #[case] end: Bound,
        #[case] expected: &str,
    ) {
        let mut repo_url = initial_repo_url.to_string();
        let current_buffer_path = Path::new(file_path);
        let selection = dummy_selection(start, end);

        build_github_file_url(&mut repo_url, url_kind, commit_hash, current_buffer_path, &selection);

        pretty_assertions::assert_eq!(repo_url, expected);
    }

    #[rstest]
    #[case::single_line_selection(
        "https://gitlab.com/user/repo",
        "blob",
        "abc123",
        "/src/main.rs",
        Bound { lnum: 10, col: 5 },
        Bound { lnum: 10, col: 10 },
        "https://gitlab.com/user/repo/-/blob/abc123/src/main.rs#L11-11"
    )]
    #[case::multi_line_selection(
        "https://gitlab.com/user/repo",
        "blob",
        "def456",
        "/lib/utils.rs",
        Bound { lnum: 1, col: 0 },
        Bound { lnum: 3, col: 20 },
        "https://gitlab.com/user/repo/-/blob/def456/lib/utils.rs#L2-4"
    )]
    #[case::root_file(
        "https://gitlab.com/user/repo",
        "tree",
        "ghi789",
        "/README.md",
        Bound { lnum: 5, col: 2 },
        Bound { lnum: 5, col: 2 },
        "https://gitlab.com/user/repo/-/tree/ghi789/README.md#L6-6"
    )]
    fn build_gitlab_file_url_works_as_expected(
        #[case] initial_repo_url: &str,
        #[case] url_kind: &str,
        #[case] commit_hash: &str,
        #[case] file_path: &str,
        #[case] start: Bound,
        #[case] end: Bound,
        #[case] expected: &str,
    ) {
        let mut repo_url = initial_repo_url.to_string();
        let current_buffer_path = Path::new(file_path);
        let selection = dummy_selection(start, end);

        build_gitlab_file_url(&mut repo_url, url_kind, commit_hash, current_buffer_path, &selection);

        pretty_assertions::assert_eq!(repo_url, expected);
    }

    fn dummy_selection(start: Bound, end: Bound) -> Selection {
        use ytil_noxi::visual_selection::SelectionBounds;
        let bounds = SelectionBounds { buf_id: 1, start, end };
        Selection::new(bounds, std::iter::empty::<nvim_oxi::String>())
    }
}
