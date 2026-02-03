//! Copy current Helix file path with line number to clipboard.
//!
//! # Errors
//! - `WezTerm` command or status line parsing fails.
#![feature(exit_status_error)]

use core::str::FromStr;
use std::process::Command;

use color_eyre::eyre::eyre;
use ytil_editor::Editor;
use ytil_hx::HxStatusLine;
use ytil_sys::cli::Args;

/// Formats Helix status line into file path with line number.
///
/// # Errors
/// - UTF-8 conversion fails.
fn format_hx_status_line(hx_status_line: &HxStatusLine) -> color_eyre::Result<String> {
    let file_path = hx_status_line
        .file_path
        .to_str()
        .ok_or_else(|| eyre!("cannot convert path to str | path={:#?}", hx_status_line.file_path))?;

    Ok(format!("{file_path}:{}", hx_status_line.position.line))
}

/// Copy current Helix file path with line number to clipboard.
fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let args = ytil_sys::cli::get();
    if args.has_help() {
        println!("{}", include_str!("../help.txt"));
        return Ok(());
    }

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
        .ok_or_else(|| eyre!("missing hx status line | pane_id={hx_pane_id} text={wezterm_pane_text:#?}"))?;

    let hx_status_line = HxStatusLine::from_str(hx_status_line_str)?;

    ytil_sys::file::cp_to_system_clipboard(&mut format_hx_status_line(&hx_status_line)?.as_bytes())?;

    Ok(())
}
