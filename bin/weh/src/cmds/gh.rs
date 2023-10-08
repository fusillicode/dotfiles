use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;
use std::str::FromStr;

use anyhow::anyhow;
use url::Url;

use crate::utils::get_current_pane_sibling_with_title;
use crate::utils::GhRepoView;
use crate::utils::HxPosition;

pub fn run<'a>(_args: impl Iterator<Item = &'a str>) -> anyhow::Result<()> {
    let get_current_git_branch = std::thread::spawn(|| -> anyhow::Result<String> {
        Ok(String::from_utf8(
            Command::new("git")
                .args(["branch", "--show-current"])
                .output()?
                .stdout,
        )?
        .trim()
        .to_owned())
    });

    let get_gh_repo_view = std::thread::spawn(|| -> anyhow::Result<GhRepoView> {
        Ok(serde_json::from_slice(
            &Command::new("gh")
                .args(["repo", "view", "--json", "url"])
                .output()?
                .stdout,
        )?)
    });

    let get_hx_position = std::thread::spawn(move || -> anyhow::Result<HxPosition> {
        let hx_pane_id = get_current_pane_sibling_with_title("hx")?.pane_id;

        let wezterm_pane_text = String::from_utf8(
            Command::new("wezterm")
                .args(["cli", "get-text", "--pane-id", &hx_pane_id.to_string()])
                .output()?
                .stdout,
        )?;

        let hx_status_line = wezterm_pane_text.lines().nth_back(1).ok_or_else(|| {
            anyhow!("missing hx status line in pane {hx_pane_id} text {wezterm_pane_text:?}")
        })?;

        HxPosition::from_str(hx_status_line)
    });

    let get_git_repo_root_dir = std::thread::spawn(|| -> anyhow::Result<String> {
        Ok(String::from_utf8(
            Command::new("git")
                .args(["rev-parse", "--show-toplevel"])
                .output()?
                .stdout,
        )?)
    });

    let link_to_github = build_link_to_github(
        std::env::current_dir()?,
        crate::utils::exec(get_git_repo_root_dir)?,
        crate::utils::exec(get_hx_position)?,
        crate::utils::exec(get_gh_repo_view)?.url,
        &crate::utils::exec(get_current_git_branch)?,
    )?;

    let mut pbcopy_child = Command::new("pbcopy").stdin(Stdio::piped()).spawn()?;
    std::io::copy(
        &mut link_to_github.to_string().as_bytes(),
        pbcopy_child
            .stdin
            .as_mut()
            .ok_or_else(|| anyhow!("cannot get child stdin as mut"))?,
    )?;
    pbcopy_child.wait()?;

    Ok(())
}

fn build_link_to_github(
    current_dir: PathBuf,
    git_repo_root_dir: String,
    hx_position: HxPosition,
    gh_repo_url: Url,
    current_git_branch: &str,
) -> anyhow::Result<Url> {
    let missing_path_part = current_dir
        .to_str()
        .ok_or_else(|| anyhow!("cannot get str from OsStr {current_dir:?}"))?
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
        .map_err(|_| anyhow!("cannot extend URL {gh_repo_url} with segments {segments:?}"))?
        .extend(&segments);
    link_to_github.set_fragment(Some(&format!(
        "L{}C{}",
        hx_position.line, hx_position.column
    )));

    Ok(link_to_github)
}
