#![feature(exit_status_error)]

use std::process::Command;
use std::str::FromStr;

use color_eyre::eyre::eyre;
use utils::editor::Editor;
use utils::hx::HxStatusLine;

/// Copies the file path of the currently active file in Helix editor to the clipboard.
///
/// This tool integrates with Wezterm and Helix to extract the file path from the
/// Helix status line and copy it to the system clipboard. The path includes the
/// line number in a format suitable for editors and IDEs.
///
/// # How it Works
///
/// 1. Detects the current Wezterm pane
/// 2. Finds a sibling pane running Helix
/// 3. Extracts the current file path from Helix status line
/// 4. Formats the path with line number (e.g., `path/to/file.rs:42`)
/// 5. Copies the formatted path to the system clipboard
///
/// # Output Format
///
/// The copied path follows the format: `file_path:line_number`
///
/// This format is compatible with most editors and tools that accept file:line
/// references for navigation.
///
/// # Examples
///
/// Copy current file path to clipboard:
/// ```bash
/// yhfp
/// ```
///
/// # Requirements
///
/// - Helix editor must be running in a Wezterm pane
/// - Wezterm must be the terminal emulator
/// - System clipboard must be accessible
///
/// # Use Cases
///
/// - Sharing file references with team members
/// - Creating quick links for code reviews
/// - Integration with other development tools
/// - Quick file path extraction for documentation
fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let hx_pane_id = utils::wezterm::get_sibling_pane_with_titles(
        &utils::wezterm::get_all_panes(&[])?,
        utils::wezterm::get_current_pane_id()?,
        Editor::Hx.pane_titles(),
    )?
    .pane_id;

    let wezterm_pane_text = String::from_utf8(
        Command::new("wezterm")
            .args(["cli", "get-text", "--pane-id", &hx_pane_id.to_string()])
            .output()?
            .stdout,
    )?;

    let hx_status_line = wezterm_pane_text
        .lines()
        .nth_back(1)
        .ok_or_else(|| eyre!("no hx status line in pane '{hx_pane_id}' text {wezterm_pane_text:#?}"))?;

    let hx_status_line = HxStatusLine::from_str(hx_status_line)?;

    utils::system::cp_to_system_clipboard(&mut format_hx_status_line(&hx_status_line)?.as_bytes())?;

    Ok(())
}

/// Formats a Helix status line into a file path with line number.
///
/// This function converts a [HxStatusLine] into a string format that includes
/// the file path and line number, suitable for use in editors and development tools.
///
/// # Arguments
///
/// * `hx_status_line` - The parsed Helix status line containing file path and cursor position
///
/// # Returns
///
/// Returns a formatted string in the format `file_path:line_number`.
///
/// # Example
///
/// For a file `/path/to/file.rs` at line 42, this returns:
/// `/path/to/file.rs:42`
fn format_hx_status_line(hx_status_line: &HxStatusLine) -> color_eyre::Result<String> {
    let file_path = hx_status_line
        .file_path
        .to_str()
        .ok_or_else(|| eyre!("cannot convert PathBuf {:#?} to str", hx_status_line.file_path))?;

    Ok(format!("{file_path}:{}", hx_status_line.position.line))
}
