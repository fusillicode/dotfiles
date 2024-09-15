use std::process::Command;
use std::str::FromStr;

use anyhow::anyhow;

use crate::cmds::open_editor::Editor;
use crate::utils::hx::HxStatusLine;

pub fn run<'a>(_args: impl Iterator<Item = &'a str>) -> anyhow::Result<()> {
    let hx_pane_id = crate::utils::wezterm::get_sibling_pane_with_titles(
        &crate::utils::wezterm::get_all_panes()?,
        std::env::var("WEZTERM_PANE")?.parse()?,
        Editor::Helix.pane_titles(),
    )?
    .pane_id;

    let wezterm_pane_text = String::from_utf8(
        Command::new("wezterm")
            .args(["cli", "get-text", "--pane-id", &hx_pane_id.to_string()])
            .output()?
            .stdout,
    )?;

    let hx_status_line = wezterm_pane_text.lines().nth_back(1).ok_or_else(|| {
        anyhow!("no hx status line in pane '{hx_pane_id}' text {wezterm_pane_text:?}")
    })?;

    let hx_status_line = HxStatusLine::from_str(hx_status_line)?;

    crate::utils::system::copy_to_system_clipboard(
        &mut format_hx_status_line(&hx_status_line)?.as_bytes(),
    )?;

    Ok(())
}

fn format_hx_status_line(hx_status_line: &HxStatusLine) -> anyhow::Result<String> {
    let file_path = hx_status_line.file_path.to_str().ok_or_else(|| {
        anyhow!(
            "cannot convert PathBuf {:?} to str",
            hx_status_line.file_path
        )
    })?;

    Ok(format!("{}:{}", file_path, hx_status_line.position.line))
}
