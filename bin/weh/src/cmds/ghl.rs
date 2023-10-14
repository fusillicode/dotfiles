use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::str::FromStr;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::bail;
use url::Url;

use crate::utils::get_current_pane_sibling_with_title;
use crate::utils::HxCursor;
use crate::utils::HxCursorPosition;

pub fn run<'a>(_args: impl Iterator<Item = &'a str>) -> anyhow::Result<()> {
    let hx_pane_id = get_current_pane_sibling_with_title("hx")?.pane_id;

    let wezterm_pane_text = String::from_utf8(
        Command::new("wezterm")
            .args(["cli", "get-text", "--pane-id", &hx_pane_id.to_string()])
            .output()?
            .stdout,
    )?;

    let hx_cursor =
        HxCursor::from_str(wezterm_pane_text.lines().nth_back(1).ok_or_else(|| {
            anyhow!("missing hx status line in pane '{hx_pane_id}' text {wezterm_pane_text:?}")
        })?)?;

    let git_repo_root = Arc::new(get_git_repo_root(&hx_cursor.file_path)?);

    let git_repo_root_clone = git_repo_root.clone();
    let get_git_current_branch = std::thread::spawn(move || -> anyhow::Result<String> {
        Ok(String::from_utf8(
            Command::new("git")
                .args(["-C", &git_repo_root_clone, "branch", "--show-current"])
                .output()?
                .stdout,
        )?
        .trim()
        .to_owned())
    });

    let git_repo_root_clone = git_repo_root.clone();
    let get_github_repo_url = std::thread::spawn(move || -> anyhow::Result<Url> {
        get_github_url_from_git_remote_output(&String::from_utf8(
            Command::new("git")
                .args(["-C", &git_repo_root_clone, "remote", "-v"])
                .output()?
                .stdout,
        )?)
    });

    // `get_relative_file_path` is before the `join`s to let them work in the background as much as possible.
    let file_path =
        get_file_path_relative_to_git_repo_root(&hx_cursor.file_path, git_repo_root.as_str())?;
    let github_repo_url = crate::utils::join(get_github_repo_url)?;
    let git_current_branch = crate::utils::join(get_git_current_branch)?;

    let github_link = build_github_link(
        &github_repo_url,
        &git_current_branch,
        &file_path,
        &hx_cursor.position,
    )?;

    crate::utils::copy_to_system_clipboard(&mut github_link.as_str().as_bytes())?;

    Ok(())
}

fn get_git_repo_root(file_path: &Path) -> anyhow::Result<String> {
    let file_parent_dir = file_path
        .parent()
        .ok_or_else(|| anyhow!("cannot get parent dir from path {file_path:?}"))?
        .to_str()
        .ok_or_else(|| anyhow!("cannot get str from Path {file_path:?}"))?
        .to_owned();

    let git_repo_root = Command::new("sh")
        .args([
            "-c",
            &format!("git -C {file_parent_dir} rev-parse --show-toplevel"),
        ])
        .output()?
        .stdout;

    if git_repo_root.is_empty() {
        bail!("{file_path:?} is not in a git repository");
    }

    Ok(String::from_utf8(git_repo_root)?.trim().to_owned())
}

fn get_github_url_from_git_remote_output(git_remote_output: &str) -> Result<Url, anyhow::Error> {
    let git_remote_fetch_line = git_remote_output
        .trim()
        .lines()
        .find(|l| l.ends_with("(fetch)"))
        .ok_or_else(|| anyhow!("no '(fetch)' line in git remote output '{git_remote_output}'"))?;

    let git_remote_url = git_remote_fetch_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| anyhow!("no git remote url in '(fetch)' line '{git_remote_fetch_line}'"))?;

    parse_github_url_from_git_remote_url(git_remote_url)
}

fn parse_github_url_from_git_remote_url(git_remote_url: &str) -> anyhow::Result<Url> {
    if let Ok(mut url) = Url::parse(git_remote_url) {
        url.set_path(url.clone().path().trim_end_matches(".git"));
        return Ok(url);
    }

    let path = git_remote_url
        .split_once(':')
        .map(|(_, path)| path.trim_end_matches(".git"))
        .ok_or_else(|| anyhow!("cannot extract URL path from '{git_remote_url}'"))?;

    let mut url = Url::parse("https://github.com")?;
    url.set_path(path);

    Ok(url)
}

// FIXME: TEST ME PLEASE!!!
fn get_file_path_relative_to_git_repo_root(
    file_path: &Path,
    git_repo_root: &str,
) -> anyhow::Result<PathBuf> {
    let file_path = file_path
        .to_str()
        .ok_or_else(|| anyhow!("cannot get str from Path {:?}", file_path))?;

    Ok(if file_path.starts_with('~') {
        file_path.replace("~", &std::env::var("HOME")?)
    } else if file_path.starts_with('/') {
        file_path.to_owned()
    } else {
        let mut current_dir = std::env::current_dir()?;
        current_dir.push(file_path);
        current_dir
            .to_str()
            .ok_or_else(|| anyhow!("cannot get str from Path {:?}", current_dir))?
            .to_owned()
    }
    .replace(&git_repo_root, "")
    .trim_start_matches(std::path::MAIN_SEPARATOR_STR)
    .into())
}

fn build_github_link<'a>(
    github_repo_url: &'a Url,
    git_current_branch: &'a str,
    file_path: &'a Path,
    hx_cursor_position: &'a HxCursorPosition,
) -> anyhow::Result<Url> {
    let file_path_parts = file_path
        .components()
        .map(|x| x.as_os_str().to_str().unwrap().to_string())
        .collect::<Vec<_>>();

    let segments = [
        &["tree", git_current_branch],
        file_path_parts
            .iter()
            .map(AsRef::as_ref)
            .collect::<Vec<_>>()
            .as_slice(),
    ]
    .concat();
    let mut github_link = github_repo_url.clone();
    github_link
        .path_segments_mut()
        .map_err(|_| anyhow!("cannot extend URL '{github_repo_url}' with segments {segments:?}"))?
        .extend(&segments);
    github_link.set_fragment(Some(&format!(
        "L{}C{}",
        hx_cursor_position.line, hx_cursor_position.column
    )));

    Ok(github_link)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_github_url_from_git_remote_output_works_as_expected_with_ssh_remotes() {
        // Arrange
        let input = r#"
            origin       git@github.com:fusillicode/dotfiles.git (fetch)
            origin  git@github.com:fusillicode/dotfiles.git (push)

        "#;

        // Act
        let result = get_github_url_from_git_remote_output(input).unwrap();

        // Assert
        let expected = Url::parse("https://github.com/fusillicode/dotfiles").unwrap();
        assert_eq!(expected, result);
    }

    #[test]
    fn test_get_github_url_from_git_remote_output_works_as_expected_with_https_remotes() {
        // Arrange
        let input = r#"
            origin       https://github.com/fusillicode/dotfiles.git (fetch)
            origin  git@github.com:fusillicode/dotfiles.git (push)
            
        "#;

        // Act
        let result = get_github_url_from_git_remote_output(input).unwrap();

        // Assert
        let expected = Url::parse("https://github.com/fusillicode/dotfiles").unwrap();
        assert_eq!(expected, result);
    }

    // #[test]
    // fn test_get_relative_file_path_works_as_expected_with_home_relative_path() {
    //     // Act
    //     let result =
    //         get_file_path_relative_to_git_repo_root(Path::new("~/foo/bar/baz.rs"), "/bar").unwrap();

    //     // Assert
    //     assert_eq!("foo", result);
    // }

    // #[test]
    // fn test_get_relative_file_path_works_as_expected_with_absolute_path() {
    //     // Act
    //     let result =
    //         get_file_path_relative_to_git_repo_root(Path::new("/foo/bar/baz.rs"), "/bar").unwrap();

    //     // Assert
    //     assert_eq!("foo", result);
    // }

    // #[test]
    // fn test_get_relative_file_path_works_as_expected_with_current_path() {
    //     // Act
    //     let result =
    //         get_file_path_relative_to_git_repo_root(Path::new("foo/bar/baz.rs"), "/bar").unwrap();

    //     // Assert
    //     assert_eq!("foo", result);
    // }
}
