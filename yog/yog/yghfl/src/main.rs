//! Copy GitHub URL (file/line/col) for the current Helix buffer to clipboard.
//!
//! # Usage
//! ```bash
//! yghfl # copies https://github.com/<org>/<repo>/tree/<branch>/<path>#L<C> to clipboard
//! ```
//!
//! # Errors
//! - `WezTerm` pane text retrieval fails.
//! - Status line extraction or parse fails.
//! - Git repository root or remote URL resolution fails.
//! - Multiple or zero GitHub remotes detected.
//! - Current branch lookup fails.
//! - Invalid UTF-8 in paths or process output.

#![feature(exit_status_error)]

use core::str::FromStr;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;

use color_eyre::eyre::bail;
use color_eyre::eyre::eyre;
use url::Url;
use ytil_editor::Editor;
use ytil_hx::HxCursorPosition;
use ytil_hx::HxStatusLine;
use ytil_system::CliArgs;
use ytil_wezterm::WeztermPane;
use ytil_wezterm::get_sibling_pane_with_titles;

/// Builds absolute file path for Helix cursor position.
///
/// # Errors
/// - Expanding a home-relative path (starting with `~`) fails because the home directory cannot be determined.
fn build_hx_cursor_absolute_file_path(
    hx_cursor_file_path: &Path,
    hx_pane: &WeztermPane,
) -> color_eyre::Result<PathBuf> {
    if let Ok(hx_cursor_file_path) = hx_cursor_file_path.strip_prefix("~") {
        return ytil_system::build_home_path(&[hx_cursor_file_path]);
    }

    let mut components = hx_pane.cwd.components();
    components.next();
    components.next();

    Ok(std::iter::once(Component::RootDir)
        .chain(components)
        .chain(hx_cursor_file_path.components())
        .collect())
}

/// Builds GitHub link pointing to specific file and line.
///
/// # Errors
/// - UTF-8 conversion fails.
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
                .ok_or_else(|| eyre!("path component invalid utf-8 | component={component:#?}"))?,
        );
    }

    let segments = [&["tree", git_current_branch], file_path_parts.as_slice()].concat();
    let mut github_link = github_repo_url.clone();
    github_link
        .path_segments_mut()
        .map_err(|()| eyre!("cannot extend url with segments | url={github_repo_url} segments={segments:#?}"))?
        .extend(&segments);
    github_link.set_fragment(Some(&format!(
        "L{}C{}",
        hx_cursor_position.line, hx_cursor_position.column
    )));

    Ok(github_link)
}

/// Copy GitHub URL (file/line/col) for the current Helix buffer to clipboard.
fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let args = ytil_system::get_args();
    if args.has_help() {
        println!("{}", include_str!("../help.txt"));
        return Ok(());
    }

    let hx_pane = get_sibling_pane_with_titles(
        &ytil_wezterm::get_all_panes(&[])?,
        ytil_wezterm::get_current_pane_id()?,
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
            "missing hx status line | pane_id={} text={wezterm_pane_text:#?}",
            hx_pane.pane_id
        )
    })?)?;

    let git_repo_root_path = Arc::new(ytil_git::get_repo_root(&ytil_git::discover_repo(
        &hx_status_line.file_path,
    )?));

    let get_git_current_branch =
        std::thread::spawn(move || -> color_eyre::Result<String> { ytil_git::branch::get_current() });

    let git_repo_root_path_clone = git_repo_root_path.clone();
    let get_github_repo_url = std::thread::spawn(move || -> color_eyre::Result<Url> {
        match &ytil_github::get_repo_urls(&git_repo_root_path_clone)?.as_slice() {
            &[] => bail!("missing GitHub repo URL | repo_path={git_repo_root_path_clone:#?}"),
            &[one] => Ok(one.clone()),
            multi => {
                bail!("multiple GitHub repo URLs | URLs={multi:#?} repo_path={git_repo_root_path_clone:#?}")
            }
        }
    });

    // `build_file_path_relative_to_git_repo_root` are called before the threads `join` to let them work in the
    // background as much as possible
    let hx_cursor_absolute_file_path = build_hx_cursor_absolute_file_path(&hx_status_line.file_path, &hx_pane)?;

    let github_link = build_github_link(
        &ytil_system::join(get_github_repo_url)?,
        &ytil_system::join(get_git_current_branch)?,
        hx_cursor_absolute_file_path.strip_prefix(git_repo_root_path.as_ref())?,
        &hx_status_line.position,
    )?;

    ytil_system::cp_to_system_clipboard(&mut github_link.as_str().as_bytes())?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use fake::Fake;
    use fake::Faker;

    use super::*;

    #[test]
    fn build_hx_cursor_absolute_file_path_works_as_expected_with_file_path_as_relative_to_home_dir() {
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
            assert_eq!(result.unwrap(), expected);
        });
    }

    #[test]
    fn build_hx_cursor_absolute_file_path_works_as_expected_with_file_path_as_relative_to_hx_root() {
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
    fn build_hx_cursor_absolute_file_path_works_as_expected_with_file_path_as_absolute() {
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
}
