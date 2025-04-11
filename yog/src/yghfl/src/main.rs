#![feature(exit_status_error)]

use std::path::Component;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::str::FromStr;
use std::sync::Arc;

use color_eyre::eyre::bail;
use color_eyre::eyre::eyre;
use url::Url;

use utils::editor::Editor;
use utils::hx::HxCursorPosition;
use utils::hx::HxStatusLine;
use utils::wezterm::get_sibling_pane_with_titles;
use utils::wezterm::WezTermPane;

/// Yank link to GitHub of the file displayed in the status line of the first Helix instance found running alongside
/// the Wezterm pane from where the cmd has been invoked.
fn main() -> color_eyre::Result<()> {
    let hx_pane = get_sibling_pane_with_titles(
        &utils::wezterm::get_all_panes()?,
        std::env::var("WEZTERM_PANE")?.parse()?,
        Editor::Helix.pane_titles(),
    )?;

    let wezterm_pane_text = String::from_utf8(
        Command::new("wezterm")
            .args(["cli", "get-text", "--pane-id", &hx_pane.pane_id.to_string()])
            .output()?
            .stdout,
    )?;

    let hx_status_line =
        HxStatusLine::from_str(wezterm_pane_text.lines().nth_back(1).ok_or_else(|| {
            eyre!(
                "no hx status line in pane '{}' text {wezterm_pane_text:?}",
                hx_pane.pane_id
            )
        })?)?;

    let git_repo_root = Arc::new(get_git_repo_root(&hx_status_line.file_path)?);

    let git_repo_root_clone = git_repo_root.clone();
    let get_git_current_branch = std::thread::spawn(move || -> color_eyre::Result<String> {
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
    let get_github_repo_url = std::thread::spawn(move || -> color_eyre::Result<Url> {
        get_github_url_from_git_remote_output(&String::from_utf8(
            Command::new("git")
                .args(["-C", &git_repo_root_clone, "remote", "-v"])
                .output()?
                .stdout,
        )?)
    });

    // `build_file_path_relative_to_git_repo_root` are called before the 🧵s `join` to let them work in the background
    // as much as possible
    let hx_cursor_absolute_file_path =
        build_hx_cursor_absolute_file_path(&hx_status_line.file_path, &hx_pane)?;

    let github_link = build_github_link(
        &utils::system::join(get_github_repo_url)?,
        &utils::system::join(get_git_current_branch)?,
        hx_cursor_absolute_file_path.strip_prefix(git_repo_root.as_ref())?,
        &hx_status_line.position,
    )?;

    utils::system::copy_to_system_clipboard(&mut github_link.as_str().as_bytes())?;

    Ok(())
}

fn get_git_repo_root(file_path: &Path) -> color_eyre::Result<String> {
    let file_parent_dir = file_path
        .parent()
        .ok_or_else(|| eyre!("cannot get parent dir from path {file_path:?}"))?
        .to_str()
        .ok_or_else(|| eyre!("cannot get str from Path {file_path:?}"))?;

    // Without spawning an additional `sh` shell I get an empty `Command` output 🥲
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

fn get_github_url_from_git_remote_output(git_remote_output: &str) -> color_eyre::Result<Url> {
    let git_remote_fetch_line = git_remote_output
        .trim()
        .lines()
        .find(|l| l.ends_with("(fetch)"))
        .ok_or_else(|| eyre!("no '(fetch)' line in git remote output '{git_remote_output}'"))?;

    let git_remote_url = git_remote_fetch_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| eyre!("no git remote url in '(fetch)' line '{git_remote_fetch_line}'"))?;

    parse_github_url_from_git_remote_url(git_remote_url)
}

fn parse_github_url_from_git_remote_url(git_remote_url: &str) -> color_eyre::Result<Url> {
    if let Ok(mut url) = Url::parse(git_remote_url) {
        url.set_path(url.clone().path().trim_end_matches(".git"));
        return Ok(url);
    }

    let path = git_remote_url
        .split_once(':')
        .map(|(_, path)| path.trim_end_matches(".git"))
        .ok_or_else(|| eyre!("cannot extract URL path from '{git_remote_url}'"))?;

    let mut url = Url::parse("https://github.com")?;
    url.set_path(path);

    Ok(url)
}

fn build_hx_cursor_absolute_file_path(
    hx_cursor_file_path: &Path,
    hx_pane: &WezTermPane,
) -> color_eyre::Result<PathBuf> {
    if let Ok(hx_cursor_file_path) = hx_cursor_file_path.strip_prefix("~") {
        let mut home_absolute_path = Path::new(&std::env::var("HOME")?).to_path_buf();
        home_absolute_path.push(hx_cursor_file_path);
        return Ok(home_absolute_path);
    }

    let mut components = hx_pane.cwd.components();
    components.next();
    components.next();

    Ok(std::iter::once(Component::RootDir)
        .chain(components)
        .chain(hx_cursor_file_path.components())
        .collect())
}

fn build_github_link<'a>(
    github_repo_url: &'a Url,
    git_current_branch: &'a str,
    file_path: &'a Path,
    hx_cursor_position: &'a HxCursorPosition,
) -> color_eyre::Result<Url> {
    let mut file_path_parts = vec![];
    for component in file_path.components() {
        file_path_parts.push(
            component
                .as_os_str()
                .to_str()
                .ok_or_else(|| eyre!("cannot get str from path component {component:?}"))?,
        );
    }

    let segments = [&["tree", git_current_branch], file_path_parts.as_slice()].concat();
    let mut github_link = github_repo_url.clone();
    github_link
        .path_segments_mut()
        .map_err(|_| eyre!("cannot extend URL '{github_repo_url}' with segments {segments:?}"))?
        .extend(&segments);
    github_link.set_fragment(Some(&format!(
        "L{}C{}",
        hx_cursor_position.line, hx_cursor_position.column
    )));

    Ok(github_link)
}

#[cfg(test)]
mod tests {
    use fake::Fake;
    use fake::Faker;

    use super::*;

    #[test]
    fn test_build_hx_cursor_absolute_file_path_works_as_expected_with_file_path_as_relative_to_home_dir(
    ) {
        // Arrange
        temp_env::with_vars([("HOME", Some("/Users/Foo"))], || {
            let hx_status_line = HxStatusLine {
                file_path: Path::new("~/src/bar/baz.rs").into(),
                ..Faker.fake()
            };
            let hx_pane = WezTermPane {
                cwd: Path::new("file://hostname/Users/Foo/dev").into(),
                ..Faker.fake()
            };

            // Act
            let result = build_hx_cursor_absolute_file_path(&hx_status_line.file_path, &hx_pane);

            // Assert
            let expected = Path::new("/Users/Foo/src/bar/baz.rs").to_path_buf();
            assert_eq!(expected, result.unwrap());
        })
    }

    #[test]
    fn test_build_hx_cursor_absolute_file_path_works_as_expected_with_file_path_as_relative_to_hx_root(
    ) {
        // Arrange
        let hx_status_line = HxStatusLine {
            file_path: Path::new("src/bar/baz.rs").into(),
            ..Faker.fake()
        };
        let hx_pane = WezTermPane {
            cwd: Path::new("file://hostname/Users/Foo/dev").into(),
            ..Faker.fake()
        };

        // Act
        let result =
            build_hx_cursor_absolute_file_path(&hx_status_line.file_path, &hx_pane).unwrap();

        // Assert
        let expected = Path::new("/Users/Foo/dev/src/bar/baz.rs").to_path_buf();
        assert_eq!(expected, result);
    }

    #[test]
    fn test_build_hx_cursor_absolute_file_path_works_as_expected_with_file_path_as_absolute() {
        // Arrange
        let hx_status_line = HxStatusLine {
            file_path: Path::new("/Users/Foo/dev/src/bar/baz.rs").into(),
            ..Faker.fake()
        };
        let hx_pane = WezTermPane {
            cwd: Path::new("file://hostname/Users/Foo/dev").into(),
            ..Faker.fake()
        };

        // Act
        let result =
            build_hx_cursor_absolute_file_path(&hx_status_line.file_path, &hx_pane).unwrap();

        // Assert
        let expected = Path::new("/Users/Foo/dev/src/bar/baz.rs").to_path_buf();
        assert_eq!(expected, result);
    }

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
