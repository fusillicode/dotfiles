//! Copy current Helix file path with line number to clipboard.
#![feature(exit_status_error)]

use core::str::FromStr;
use std::process::Command;

use color_eyre::eyre::eyre;
use ytil_editor::Editor;
use ytil_hx::HxStatusLine;

/// Copy the current Helix file path and line number to the clipboard.
///
/// # Usage
///
/// ```bash
/// yhfp    # copies 'relative/path/to/file:LINE' to clipboard
/// ```
///
/// # Errors
/// In case:
/// - Executing `wezterm` fails or returns a non-zero exit status.
/// - UTF-8 conversion fails.
fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let hx_pane_id = ytil_wezterm::get_sibling_pane_with_titles(
        &ytil_wezterm::get_all_panes(&[])?,
        ytil_wezterm::get_current_pane_id()?,
        Editor::Hx.pane_titles(),
    )?
    .pane_id;

    let wezterm_pane_text = String::from_utf8(
        Command::new("wezterm")
            .args(["cli", "get-text", "--pane-id", &hx_pane_id.to_string()])
            .output()?
            .stdout,
    )?;

    let hx_status_line_str = wezterm_pane_text
        .lines()
        .nth_back(1)
        .ok_or_else(|| eyre!("missing hx status line in pane '{hx_pane_id}' text {wezterm_pane_text:#?}"))?;

    let hx_status_line = HxStatusLine::from_str(hx_status_line_str)?;

    ytil_system::cp_to_system_clipboard(&mut format_hx_status_line(&hx_status_line)?.as_bytes())?;

    Ok(())
}

/// Formats Helix status line into file path with line number.
///
/// # Errors
/// In case:
/// - UTF-8 conversion fails.
fn format_hx_status_line(hx_status_line: &HxStatusLine) -> color_eyre::Result<String> {
    let file_path = hx_status_line
        .file_path
        .to_str()
        .ok_or_else(|| eyre!("cannot convert PathBuf {:#?} to str", hx_status_line.file_path))?;

    Ok(format!("{file_path}:{}", hx_status_line.position.line))
}
