//! Copy current Helix file path with line number to clipboard.
//!
//! # Errors
//! - `WezTerm` command or status line parsing fails.
#![feature(exit_status_error)]

use core::str::FromStr;
use std::process::Command;

use rootcause::prelude::ResultExt as _;
use rootcause::report;
use ytil_editor::Editor;
use ytil_hx::HxStatusLine;
use ytil_sys::cli::Args;

/// Formats Helix status line into file path with line number.
///
/// # Errors
/// - UTF-8 conversion fails.
fn format_hx_status_line(hx_status_line: &HxStatusLine) -> rootcause::Result<String> {
    let file_path = hx_status_line
        .file_path
        .to_str()
        .ok_or_else(|| report!("cannot convert path to str"))
        .attach_with(|| format!("path={}", hx_status_line.file_path.display()))?;

    Ok(format!("{file_path}:{}", hx_status_line.position.line))
}

/// Copy current Helix file path with line number to clipboard.
fn main() -> rootcause::Result<()> {
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
        .ok_or_else(|| report!("missing hx status line"))
        .attach_with(|| format!("pane_id={hx_pane_id} text={wezterm_pane_text:#?}"))?;

    let hx_status_line = HxStatusLine::from_str(hx_status_line_str)?;

    ytil_sys::file::cp_to_system_clipboard(&mut format_hx_status_line(&hx_status_line)?.as_bytes())?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case("src/main.rs", 42, 7, "src/main.rs:42")]
    #[case("yog/ytil/cmd/src/lib.rs", 1, 1, "yog/ytil/cmd/src/lib.rs:1")]
    #[case("file.txt", 100, 50, "file.txt:100")]
    fn format_hx_status_line_returns_path_with_line_number(
        #[case] path: &str,
        #[case] line: usize,
        #[case] column: usize,
        #[case] expected: &str,
    ) {
        let status = HxStatusLine {
            file_path: PathBuf::from(path),
            position: ytil_hx::HxCursorPosition { line, column },
        };
        assert2::assert!(let Ok(result) = format_hx_status_line(&status));
        pretty_assertions::assert_eq!(result, expected);
    }
}
