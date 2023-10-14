use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;
use std::str::FromStr;
use std::thread::JoinHandle;

use anyhow::anyhow;
use anyhow::bail;
use serde::Deserialize;

pub fn join<T>(join_handle: JoinHandle<anyhow::Result<T>>) -> Result<T, anyhow::Error> {
    join_handle
        .join()
        .map_err(|e| anyhow!("join error {e:?}"))?
}

pub fn copy_to_system_clipboard(content: &mut &[u8]) -> anyhow::Result<()> {
    let mut pbcopy_child = Command::new("pbcopy").stdin(Stdio::piped()).spawn()?;
    std::io::copy(
        content,
        pbcopy_child
            .stdin
            .as_mut()
            .ok_or_else(|| anyhow!("cannot get child stdin as mut"))?,
    )?;
    if !pbcopy_child.wait()?.success() {
        bail!("error copy content to system clipboard, content {content:?}");
    }
    Ok(())
}

pub fn get_current_pane_sibling_with_title(pane_title: &str) -> anyhow::Result<WezTermPane> {
    let current_pane_id: i64 = std::env::var("WEZTERM_PANE")?.parse()?;

    let all_panes: Vec<WezTermPane> = serde_json::from_slice(
        &Command::new("wezterm")
            .args(["cli", "list", "--format", "json"])
            .output()?
            .stdout,
    )?;

    let current_pane_tab_id = all_panes
        .iter()
        .find(|w| w.pane_id == current_pane_id)
        .ok_or_else(|| {
            anyhow!("current pane id '{current_pane_id}' not found among panes {all_panes:?}")
        })?
        .tab_id;

    Ok(all_panes
        .iter()
        .find(|w| w.tab_id == current_pane_tab_id && w.title == pane_title)
        .ok_or_else(|| {
            anyhow!("pane with title '{pane_title}' not found in tab '{current_pane_tab_id}'")
        })?
        .clone())
}

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct WezTermPane {
    window_id: i64,
    tab_id: i64,
    pub pane_id: i64,
    workspace: String,
    size: Size,
    title: String,
    cwd: String,
    cursor_x: i64,
    cursor_y: i64,
    cursor_shape: String,
    cursor_visibility: String,
    left_col: i64,
    top_row: i64,
    tab_title: String,
    window_title: String,
    is_active: bool,
    is_zoomed: bool,
    tty_name: String,
}

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct Size {
    rows: i64,
    cols: i64,
    pixel_width: i64,
    pixel_height: i64,
    dpi: i64,
}

#[derive(Debug, PartialEq)]
pub struct HxCursor {
    pub file_path: PathBuf,
    pub position: HxCursorPosition,
}

impl FromStr for HxCursor {
    type Err = anyhow::Error;

    fn from_str(hx_status_line: &str) -> Result<Self, Self::Err> {
        let hx_status_line = hx_status_line.trim();

        let elements: Vec<&str> = hx_status_line.split_ascii_whitespace().collect();

        let path_left_separator_idx = elements.iter().position(|x| x == &"`").ok_or_else(|| {
            anyhow!("missing left path separator in status line elements {elements:?}")
        })?;
        let path_right_separator_idx =
            elements.iter().rposition(|x| x == &"`").ok_or_else(|| {
                anyhow!("missing right path separator in status line elements {elements:?}")
            })?;

        let &["`", path] = &elements[path_left_separator_idx..path_right_separator_idx] else {
            bail!("missing path in status line elements {elements:?}");
        };

        Ok(Self {
            file_path: path.into(),
            position: HxCursorPosition::from_str(elements.last().ok_or_else(|| {
                anyhow!("missing last element in status line elements {elements:?}")
            })?)?,
        })
    }
}

#[derive(Debug, PartialEq)]
pub struct HxCursorPosition {
    pub line: i64,
    pub column: i64,
}

impl FromStr for HxCursorPosition {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (line, column) = s
            .split_once(':')
            .ok_or_else(|| anyhow!("no line column delimiter found in str '{s}'"))?;

        Ok(Self {
            line: line.parse()?,
            column: column.parse()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hx_cursor_from_str_works_as_expected_with_a_file_path_pointing_to_an_existent_file_in_normal_mode(
    ) {
        let result = HxCursor::from_str("      ● 1 ` src/utils.rs `                                                                  1 sel  1 char  W ● 1  42:33 ");
        let expected = HxCursor {
            file_path: "src/utils.rs".into(),
            position: HxCursorPosition {
                line: 42,
                column: 33,
            },
        };

        assert_eq!(expected, result.unwrap());
    }

    #[test]
    fn test_hx_cursor_from_str_works_as_expected_with_a_file_path_pointing_to_an_existent_file_and_a_spinner(
    ) {
        let result = HxCursor::from_str("⣷      ` src/utils.rs `                                                                  1 sel  1 char  W ● 1  33:42 ");
        let expected = HxCursor {
            file_path: "src/utils.rs".into(),
            position: HxCursorPosition {
                line: 33,
                column: 42,
            },
        };

        assert_eq!(expected, result.unwrap());
    }
}
