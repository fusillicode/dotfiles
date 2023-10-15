use std::process::Command;
use std::str::FromStr;

use anyhow::anyhow;

use crate::utils::get_current_pane_sibling_with_title;
use crate::utils::HxCursor;

pub fn run<'a>(_args: impl Iterator<Item = &'a str>) -> anyhow::Result<()> {
    let hx_pane_id = get_current_pane_sibling_with_title("hx")?.pane_id;

    let wezterm_pane_text = String::from_utf8(
        Command::new("wezterm")
            .args(["cli", "get-text", "--pane-id", &hx_pane_id.to_string()])
            .output()?
            .stdout,
    )?;

    let hx_status_line = wezterm_pane_text.lines().nth_back(1).ok_or_else(|| {
        anyhow!("no hx status line in pane '{hx_pane_id}' text {wezterm_pane_text:?}")
    })?;

    let hx_cursor = HxCursor::from_str(hx_status_line)?;

    crate::utils::copy_to_system_clipboard(&mut format_hx_cursor(&hx_cursor)?.as_bytes())?;

    Ok(())
}

fn format_hx_cursor(hx_cursor: &HxCursor) -> anyhow::Result<String> {
    let file_path = hx_cursor
        .file_path
        .to_str()
        .ok_or_else(|| anyhow!("cannot convert PathBuf {:?} to str", hx_cursor.file_path))?;

    Ok(format!("{}:{}", file_path, hx_cursor.position.line))
}
