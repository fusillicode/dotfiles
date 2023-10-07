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

    let mut link_to_github = gh_repo_view.url;
    link_to_github.set_path(&format!(
        "tree/{}/{}",
        current_git_branch, hx_position.file_path
    ));
    link_to_github.set_fragment(Some(&format!(
        "L{}C{}",
        hx_position.line, hx_position.column
    )));

    crate::utils::new_sh_cmd(&format!("echo '{}' | pbcopy", link_to_github))
        .output()
        .unwrap();

    Ok(())
}
