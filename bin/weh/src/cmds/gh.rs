use std::path::PathBuf;
use std::str::FromStr;

use crate::utils::get_current_pane_sibling_with_title;
use crate::utils::GhRepoView;
use crate::utils::HxPosition;

pub fn run<'a>(_args: impl Iterator<Item = &'a str>) -> Result<(), anyhow::Error> {
    let hx_pane = get_current_pane_sibling_with_title("hx")?;

    let current_git_branch = String::from_utf8(
        crate::utils::new_sh_cmd("git branch --show-current")
            .output()?
            .stdout,
    )?;
    let current_git_branch = current_git_branch.trim();

    let gh_repo_view: GhRepoView = serde_json::from_slice(
        &crate::utils::new_sh_cmd("gh repo view --json url")
            .output()?
            .stdout,
    )?;

    let wezterm_pane_text = String::from_utf8(
        crate::utils::new_sh_cmd(&format!(
            "wezterm cli get-text --pane-id {}",
            hx_pane.pane_id
        ))
        .output()?
        .stdout,
    )?;

    let hx_status_line = wezterm_pane_text.lines().nth_back(1).ok_or_else(|| {
        anyhow::anyhow!(
            "cannot get hx status line from pane text {:?}",
            wezterm_pane_text
        )
    })?;

    let hx_position = HxPosition::from_str(hx_status_line)?;

    let current_dir = std::env::current_dir()?;
    let git_repo_root_dir = String::from_utf8(
        crate::utils::new_sh_cmd("git rev-parse --show-toplevel")
            .output()?
            .stdout,
    )?;

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

    let mut link_to_github = gh_repo_view.url;
    let segments = [&["tree", current_git_branch], file_path_parts.as_slice()].concat();
    link_to_github
        .clone()
        .path_segments_mut()
        .map_err(|_| {
            anyhow::anyhow!(
                "cannot extend URL {} with segments {:?}",
                link_to_github,
                segments
            )
        })?
        .extend(&segments);
    link_to_github.set_fragment(Some(&format!(
        "L{}C{}",
        hx_position.line, hx_position.column
    )));

    crate::utils::new_sh_cmd(&format!("echo '{}' | pbcopy", link_to_github.as_str())).output()?;

    Ok(())
}
