use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::str::FromStr;
use std::sync::Arc;

use anyhow::anyhow;
use url::Url;

use crate::utils::get_current_pane_sibling_with_title;
use crate::utils::HxCursorPosition;

pub fn run<'a>(_args: impl Iterator<Item = &'a str>) -> anyhow::Result<()> {
    let hx_pane_id = get_current_pane_sibling_with_title("hx")?.pane_id;

    let wezterm_pane_text = String::from_utf8(
        Command::new("wezterm")
            .args(["cli", "get-text", "--pane-id", &hx_pane_id.to_string()])
            .output()?
            .stdout,
    )?;

    let hx_cursor_position =
        HxCursorPosition::from_str(wezterm_pane_text.lines().nth_back(1).ok_or_else(|| {
            anyhow!("missing hx status line in pane '{hx_pane_id}' text {wezterm_pane_text:?}")
        })?)?;

    let file_parent_dir = dbg!(get_parent_dir(&hx_cursor_position.file_path)?.to_owned());
    let git_repo_root = Arc::new(
        String::from_utf8(
            Command::new("sh")
                .args([
                    "-c",
                    &format!("git -C {file_parent_dir} rev-parse --show-toplevel"),
                ])
                .output()?
                .stdout,
        )?
        .trim()
        .to_owned(),
    );

    let git_repo_root_clone = dbg!(git_repo_root.clone());
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

    let link_to_github = build_link_to_github(
        std::env::current_dir()?,
        git_repo_root.to_string(),
        hx_cursor_position,
        crate::utils::exec(get_github_repo_url)?,
        &crate::utils::exec(get_git_current_branch)?,
    )?;

    crate::utils::copy_to_system_clipboard(&mut link_to_github.as_str().as_bytes())?;

    Ok(())
}

fn get_parent_dir(path: &Path) -> Result<&str, anyhow::Error> {
    path.parent()
        .ok_or_else(|| anyhow!("cannot get parent dir from path {path:?}"))?
        .to_str()
        .ok_or_else(|| anyhow!("cannot get str from Path {path:?}"))
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

fn build_link_to_github(
    current_dir: PathBuf,
    git_repo_root: String,
    hx_cursor_position: HxCursorPosition,
    github_repo_url: Url,
    git_current_branch: &str,
) -> anyhow::Result<Url> {
    let missing_path_part = current_dir
        .to_str()
        .ok_or_else(|| anyhow!("cannot get str from PathBuf {current_dir:?}"))?
        .replace(git_repo_root.trim(), "");

    let mut missing_path_part: PathBuf = missing_path_part.trim_start_matches('/').into();
    missing_path_part.push(hx_cursor_position.file_path.as_path());

    let file_path_parts = missing_path_part
        .iter()
        .map(|x| {
            x.to_str()
                .unwrap_or_else(|| panic!("cannot get str from OsStr {:?}", x))
        })
        .collect::<Vec<_>>();

    let mut link_to_github = github_repo_url.clone();
    let segments = [&["tree", git_current_branch], file_path_parts.as_slice()].concat();
    link_to_github
        .path_segments_mut()
        .map_err(|_| anyhow!("cannot extend URL '{github_repo_url}' with segments {segments:?}"))?
        .extend(&segments);
    link_to_github.set_fragment(Some(&hx_cursor_position.as_github_url_segment()));

    Ok(link_to_github)
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
}
