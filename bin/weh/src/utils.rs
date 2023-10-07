use std::process::Command;
use std::str::FromStr;

use serde::Deserialize;
use url::Url;

pub fn new_sh_cmd(cmd: &str) -> Command {
    let mut sh = Command::new("sh");
    sh.args(["-c", cmd]);
    sh
}

pub fn get_current_pane_sibling_with_title(pane_title: &str) -> WezTermPane {
    let current_pane_id: i64 = std::env::var("WEZTERM_PANE").unwrap().parse().unwrap();

    let all_panes: Vec<WezTermPane> = serde_json::from_slice(
        &new_sh_cmd("wezterm cli list --format json")
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();

    let current_pane = all_panes
        .iter()
        .find(|w| w.pane_id == current_pane_id)
        .unwrap();

    all_panes
        .iter()
        .find(|w| w.tab_id == current_pane.tab_id && w.title == pane_title)
        .unwrap()
        .clone()
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

#[derive(Debug, Deserialize)]
pub struct GhRepoView {
    pub url: Url,
}

#[derive(Debug, PartialEq)]
pub struct HxPosition {
    pub file_path: String,
    pub line: i64,
    pub column: i64,
}

impl FromStr for HxPosition {
    type Err = anyhow::Error;

    fn from_str(hx_status_line: &str) -> Result<Self, Self::Err> {
        let hx_status_line = hx_status_line.trim();

        let elements: Vec<&str> = hx_status_line.split_ascii_whitespace().collect();

        let path_left_separator_idx = elements.iter().position(|x| x == &"`").unwrap();
        let path_right_separator_idx = elements.iter().rposition(|x| x == &"`").unwrap();

        let &["`", path] = &elements[path_left_separator_idx..path_right_separator_idx] else {
            anyhow::bail!("BOOM");
        };

        let HxLineColumn { line, column } =
            HxLineColumn::from_str(elements.last().ok_or_else(|| anyhow::anyhow!("BOOM"))?)?;

        Ok(Self {
            file_path: path.into(),
            line,
            column,
        })
    }
}

#[derive(Debug, PartialEq)]
pub struct HxLineColumn {
    line: i64,
    column: i64,
}

impl FromStr for HxLineColumn {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (line, column) = s.split_once(':').ok_or_else(|| anyhow::anyhow!("BOOM"))?;

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
    fn test_foo_from_str_works_as_expected() {
        let result = HxPosition::from_str("      ● 1 ` bin/weh/src/main.rs `                                                                  1 sel  1 char  W ● 1  42:33 ");
        let expected = HxPosition {
            file_path: "bin/weh/src/main.rs".into(),
            line: 42,
            column: 33,
        };

        assert_eq!(expected, result.unwrap());

        let result = HxPosition::from_str("⣷      ` bin/weh/src/main.rs `                                                                  1 sel  1 char  W ● 1  33:42 ");
        let expected = HxPosition {
            file_path: "bin/weh/src/main.rs".into(),
            line: 33,
            column: 42,
        };

        assert_eq!(expected, result.unwrap());
    }
}
