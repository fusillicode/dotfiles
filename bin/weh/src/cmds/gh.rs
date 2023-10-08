use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;
use std::str::FromStr;

use url::Url;

use crate::utils::get_current_pane_sibling_with_title;
use crate::utils::GhRepoView;
use crate::utils::HxPosition;

pub fn run<'a>(_args: impl Iterator<Item = &'a str>) -> Result<(), anyhow::Error> {
    let hx_pane = get_current_pane_sibling_with_title("hx")?;

    let current_git_branch = String::from_utf8(
        Command::new("git")
            .args(["branch", "--show-current"])
            .output()?
            .stdout,
    )?
    .trim()
    .to_owned();

    let gh_repo_view: GhRepoView = serde_json::from_slice(
        &Command::new("gh")
            .args(["repo", "view", "--json", "url"])
            .output()?
            .stdout,
    )?;

    let wezterm_pane_text = String::from_utf8(
        Command::new("wezterm")
            .args(["cli", "get-text", "--pane-id", &hx_pane.pane_id.to_string()])
            .output()?
            .stdout,
    )?;

    let hx_status_line = wezterm_pane_text.lines().nth_back(1).ok_or_else(|| {
        anyhow::anyhow!(
            "missing hx status line in pane text {:?}",
            wezterm_pane_text
        )
    })?;

    let hx_position = HxPosition::from_str(hx_status_line)?;

    let current_dir = std::env::current_dir()?;
    let git_repo_root_dir = String::from_utf8(
        Command::new("git")
            .args(["rev-parse", "--show-toplevel"])
            .output()?
            .stdout,
    )?;

    let link_to_github = build_link_to_github(
        current_dir,
        git_repo_root_dir,
        hx_position,
        gh_repo_view.url,
        &current_git_branch,
    )?;

    let mut copy_child = Command::new("pbcopy").stdin(Stdio::piped()).spawn()?;
    std::io::copy(
        &mut link_to_github.to_string().as_bytes(),
        copy_child
            .stdin
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("cannot get copy_child stdin as mut"))?,
    )?;
    copy_child.wait()?;

    Ok(())
}

fn build_link_to_github(
    current_dir: PathBuf,
    git_repo_root_dir: String,
    hx_position: HxPosition,
    gh_repo_url: Url,
    current_git_branch: &str,
) -> Result<url::Url, anyhow::Error> {
    let missing_path_part = current_dir
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("cannot get str from OsStr {:?}", current_dir))?
        .replace(git_repo_root_dir.trim(), "");

    let mut missing_path_part: PathBuf = missing_path_part.trim_start_matches('/').into();
    missing_path_part.push(hx_position.file_path);

    let file_path_parts = missing_path_part
        .iter()
        .map(|x| {
            x.to_str()
                .unwrap_or_else(|| panic!("cannot get str from OsStr {:?}", x))
        })
        .collect::<Vec<_>>();

    let mut link_to_github = gh_repo_url.clone();
    let segments = [&["tree", current_git_branch], file_path_parts.as_slice()].concat();
    link_to_github
        .path_segments_mut()
        .map_err(|_| {
            anyhow::anyhow!(
                "cannot extend URL {} with segments {:?}",
                gh_repo_url,
                segments
            )
        })?
        .extend(&segments);
    link_to_github.set_fragment(Some(&format!(
        "L{}C{}",
        hx_position.line, hx_position.column
    )));

    Ok(link_to_github)
}
