use std::path::PathBuf;
use std::str::FromStr;

use crate::utils::get_current_pane_sibling_with_title;
use crate::utils::GhRepoView;
use crate::utils::HxPosition;

pub fn run<'a>(_args: impl Iterator<Item = &'a str>) -> Result<(), anyhow::Error> {
    let hx_pane = get_current_pane_sibling_with_title("hx");

    let current_git_branch = String::from_utf8(
        crate::utils::new_sh_cmd("git branch --show-current")
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    let current_git_branch = current_git_branch.trim();

    let gh_repo_view: GhRepoView = serde_json::from_slice(
        &crate::utils::new_sh_cmd("gh repo view --json url")
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();

    let wezterm_pane_text = String::from_utf8(
        crate::utils::new_sh_cmd(&format!(
            "wezterm cli get-text --pane-id {}",
            hx_pane.pane_id
        ))
        .output()
        .unwrap()
        .stdout,
    )
    .unwrap();

    let hx_status_line = wezterm_pane_text.lines().nth_back(1).unwrap();

    let hx_position = HxPosition::from_str(hx_status_line).unwrap();

    let current_dir = std::env::current_dir().unwrap();
    let git_repo_root_dir = String::from_utf8(
        crate::utils::new_sh_cmd("git rev-parse --show-toplevel")
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();

    let missing_path_part = current_dir
        .to_str()
        .unwrap()
        .replace(git_repo_root_dir.trim(), "");

    let mut missing_path_part: PathBuf = missing_path_part.trim_start_matches('/').into();
    missing_path_part.push(hx_position.file_path);

    let file_path_parts = missing_path_part
        .iter()
        .map(|x| x.to_str().unwrap())
        .collect::<Vec<_>>();

    let mut link_to_github = gh_repo_view.url;
    link_to_github
        .path_segments_mut()
        .unwrap()
        .extend([&["tree", current_git_branch], file_path_parts.as_slice()].concat());
    link_to_github.set_fragment(Some(&format!(
        "L{}C{}",
        hx_position.line, hx_position.column
    )));

    crate::utils::new_sh_cmd(&format!("echo '{}' | pbcopy", link_to_github.as_str()))
        .output()
        .unwrap();

    Ok(())
}
