#![feature(exit_status_error)]

use std::process::Command;
use std::str::FromStr;

use color_eyre::eyre::eyre;

use utils::editor::Editor;
use utils::hx::HxStatusLine;

/// Yank file path displayed in the status line of the first Helix instance found running alongside
/// the Wezterm pane from where the cmd has been invoked.
fn main() -> color_eyre::Result<()> {
    let hx_pane_id = utils::wezterm::get_sibling_pane_with_titles(
        &utils::wezterm::get_all_panes()?,
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
        eyre!("no hx status line in pane '{hx_pane_id}' text {wezterm_pane_text:?}")
    })?;

    let hx_status_line = HxStatusLine::from_str(hx_status_line)?;

    utils::system::copy_to_system_clipboard(
        &mut format_hx_status_line(&hx_status_line)?.as_bytes(),
    )?;

    Ok(())
}

fn format_hx_status_line(hx_status_line: &HxStatusLine) -> color_eyre::Result<String> {
    let file_path = hx_status_line.file_path.to_str().ok_or_else(|| {
        eyre!(
            "cannot convert PathBuf {:?} to str",
            hx_status_line.file_path
        )
    })?;

    Ok(format!("{}:{}", file_path, hx_status_line.position.line))
}
