use std::path::PathBuf;
use std::process::Command;
use std::str::FromStr;

use anyhow::anyhow;

use crate::utils::get_current_pane_sibling_with_title;
use crate::utils::HxPosition;

pub fn run<'a>(_args: impl Iterator<Item = &'a str>) -> anyhow::Result<()> {
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

    let file_path = build_relative_file_path(
        std::env::current_dir()?,
        crate::utils::exec(get_git_repo_root_dir)?,
        crate::utils::exec(get_hx_position)?,
    )?;

    crate::utils::copy_to_system_clipboard(
        &mut file_path
            .to_str()
            .ok_or_else(|| anyhow!("cannot convert PathBuf {file_path:?} to str"))?
            .as_bytes(),
    )?;

    Ok(())
}

fn build_relative_file_path(
    current_dir: PathBuf,
    git_repo_root_dir: String,
    hx_position: HxPosition,
) -> anyhow::Result<PathBuf> {
    let missing_path_part = current_dir
        .to_str()
        .ok_or_else(|| anyhow!("cannot get str from OsStr {current_dir:?}"))?
        .replace(git_repo_root_dir.trim(), "");

    let mut missing_path_part: PathBuf = missing_path_part.trim_start_matches('/').into();
    missing_path_part.push(hx_position.file_path.as_path());

    Ok(missing_path_part)
}
