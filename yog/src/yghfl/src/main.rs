#![feature(exit_status_error)]

use std::path::Component;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::str::FromStr;
use std::sync::Arc;

use color_eyre::eyre::eyre;
use url::Url;
use utils::editor::Editor;
use utils::hx::HxCursorPosition;
use utils::hx::HxStatusLine;
use utils::wezterm::WeztermPane;
use utils::wezterm::get_sibling_pane_with_titles;

/// Generates and copies GitHub links for files currently open in Helix editor.
///
/// This tool integrates with Wezterm and Helix to create GitHub links pointing to
/// the current file and cursor position in the Helix editor. The generated link
/// includes line and column information for precise navigation.
///
/// # How it Works
///
/// 1. Detects the current Wezterm pane
/// 2. Finds a sibling pane running Helix
/// 3. Extracts the current file path and cursor position from Helix status line
/// 4. Determines the Git repository root and current branch
/// 5. Parses the Git remote URL to construct the GitHub URL
/// 6. Builds a GitHub link with file path and cursor position
/// 7. Copies the link to the system clipboard
///
/// # Supported Git Hosts
///
/// - GitHub (github.com)
/// - GitHub Enterprise instances
/// - SSH and HTTPS Git URLs
///
/// # Link Format
///
/// The generated link follows this format:
/// `https://github.com/user/repo/blob/branch/path/to/file#LlineCcolumn`
///
/// # Examples
///
/// Generate GitHub link for current file:
/// ```bash
/// yghfl
/// ```
///
/// # Requirements
///
/// - Helix editor must be running in a Wezterm pane
/// - Current directory must be within a Git repository
/// - Git remote must point to a GitHub repository
/// - Wezterm must be the terminal emulator
///
/// # Integration
///
/// This tool is designed to work seamlessly with Wezterm's pane management
/// and Helix's status line format for accurate file and position detection.
fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let hx_pane = get_sibling_pane_with_titles(
        &utils::wezterm::get_all_panes(&[])?,
        utils::wezterm::get_current_pane_id()?,
        Editor::Hx.pane_titles(),
    )?;

    let wezterm_pane_text = String::from_utf8(
        Command::new("wezterm")
            .args(["cli", "get-text", "--pane-id", &hx_pane.pane_id.to_string()])
            .output()?
            .stdout,
    )?;

    let hx_status_line = HxStatusLine::from_str(wezterm_pane_text.lines().nth_back(1).ok_or_else(|| {
        eyre!(
            "no hx status line in pane '{}' text {wezterm_pane_text:#?}",
            hx_pane.pane_id
        )
    })?)?;

    let git_repo_root = Arc::new(utils::git::get_repo_root(Some(&hx_status_line.file_path))?);

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
    let hx_cursor_absolute_file_path = build_hx_cursor_absolute_file_path(&hx_status_line.file_path, &hx_pane)?;

    let github_link = build_github_link(
        &utils::system::join(get_github_repo_url)?,
        &utils::system::join(get_git_current_branch)?,
        hx_cursor_absolute_file_path.strip_prefix(git_repo_root.as_ref())?,
        &hx_status_line.position,
    )?;

    utils::system::cp_to_system_clipboard(&mut github_link.as_str().as_bytes())?;

    Ok(())
}

/// Extracts the GitHub repository URL from Git remote output.
///
/// This function parses the output of `git remote -v` to find the fetch URL
/// and converts it to a proper GitHub HTTPS URL.
///
/// # Arguments
///
/// * `git_remote_output` - The output from `git remote -v` command
///
/// # Returns
///
/// Returns a [Url] pointing to the GitHub repository.
///
/// # Errors
///
/// Returns an error if:
/// - No fetch remote is found in the output
/// - The remote URL cannot be parsed
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

/// Converts a Git remote URL to a GitHub HTTPS URL.
///
/// This function handles both HTTPS and SSH Git remote URLs, converting them
/// to the corresponding GitHub HTTPS URL. It also removes the `.git` suffix
/// from the path.
///
/// # Arguments
///
/// * `git_remote_url` - The Git remote URL (HTTPS or SSH format)
///
/// # Returns
///
/// Returns a [Url] pointing to the GitHub repository in HTTPS format.
///
/// # Examples
///
/// - `git@github.com:user/repo.git` → `https://github.com/user/repo`
/// - `https://github.com/user/repo.git` → `https://github.com/user/repo`
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

/// Builds the absolute file path for the Helix cursor position.
///
/// This function converts a potentially relative file path from Helix's status line
/// into an absolute path. It handles different path formats:
/// - Paths starting with `~` are resolved relative to the home directory
/// - Relative paths are resolved relative to the pane's current working directory
/// - Absolute paths are returned as-is
///
/// # Arguments
///
/// * `hx_cursor_file_path` - The file path from Helix status line
/// * `hx_pane` - The Wezterm pane information containing the current working directory
///
/// # Returns
///
/// Returns a [PathBuf] containing the absolute file path.
fn build_hx_cursor_absolute_file_path(
    hx_cursor_file_path: &Path,
    hx_pane: &WeztermPane,
) -> color_eyre::Result<PathBuf> {
    if let Ok(hx_cursor_file_path) = hx_cursor_file_path.strip_prefix("~") {
        return utils::system::build_home_path(&[hx_cursor_file_path]);
    }

    let mut components = hx_pane.cwd.components();
    components.next();
    components.next();

    Ok(std::iter::once(Component::RootDir)
        .chain(components)
        .chain(hx_cursor_file_path.components())
        .collect())
}

/// Builds a GitHub link pointing to a specific file and line in the repository.
///
/// This function constructs a GitHub URL that points to a specific file at a specific
/// line and column position in the repository. The URL format follows GitHub's
/// standard for linking to source code locations.
///
/// # Arguments
///
/// * `github_repo_url` - The base GitHub repository URL
/// * `git_current_branch` - The current Git branch name
/// * `file_path` - The relative path to the file within the repository
/// * `hx_cursor_position` - The cursor position (line and column) in the file
///
/// # Returns
///
/// Returns a [Url] that links directly to the specified file and position on GitHub.
///
/// # Example
///
/// For a file `src/main.rs` at line 42, column 15 on branch `main` in repository
/// `https://github.com/user/repo`, this would generate:
/// `https://github.com/user/repo/blob/main/src/main.rs#L42C15`
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
                .ok_or_else(|| eyre!("cannot get str from path component {component:#?}"))?,
        );
    }

    let segments = [&["tree", git_current_branch], file_path_parts.as_slice()].concat();
    let mut github_link = github_repo_url.clone();
    github_link
        .path_segments_mut()
        .map_err(|_| eyre!("cannot extend URL '{github_repo_url}' with segments {segments:#?}"))?
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
    fn test_build_hx_cursor_absolute_file_path_works_as_expected_with_file_path_as_relative_to_home_dir() {
        // Arrange
        temp_env::with_vars([("HOME", Some("/Users/Foo"))], || {
            let hx_status_line = HxStatusLine {
                file_path: Path::new("~/src/bar/baz.rs").into(),
                ..Faker.fake()
            };
            let hx_pane = WeztermPane {
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
    fn test_build_hx_cursor_absolute_file_path_works_as_expected_with_file_path_as_relative_to_hx_root() {
        // Arrange
        let hx_status_line = HxStatusLine {
            file_path: Path::new("src/bar/baz.rs").into(),
            ..Faker.fake()
        };
        let hx_pane = WeztermPane {
            cwd: Path::new("file://hostname/Users/Foo/dev").into(),
            ..Faker.fake()
        };

        // Act
        let result = build_hx_cursor_absolute_file_path(&hx_status_line.file_path, &hx_pane).unwrap();

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
        let hx_pane = WeztermPane {
            cwd: Path::new("file://hostname/Users/Foo/dev").into(),
            ..Faker.fake()
        };

        // Act
        let result = build_hx_cursor_absolute_file_path(&hx_status_line.file_path, &hx_pane).unwrap();

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
