use std::process::Command;
use std::str::FromStr;

use serde::Deserialize;
use serde::Serialize;

fn main() {
    let wezterm_panes: Vec<WezTermPane> = serde_json::from_slice(
        &new_sh_cmd(&["wezterm cli list --format json"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();

    let current_pane_id: i64 = std::env::var("WEZTERM_PANE").unwrap().parse().unwrap();

    let current_tab = wezterm_panes
        .iter()
        .find(|w| w.pane_id == current_pane_id)
        .unwrap();

    let hx_pane = wezterm_panes
        .iter()
        .find(|w| w.tab_id == current_tab.tab_id && w.title == "hx")
        .unwrap();

    let current_git_branch = String::from_utf8(
        new_sh_cmd(&["git branch --show-current"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    let current_git_branch = current_git_branch.trim();

    let gh_repo_view: GhRepoView = serde_json::from_slice(
        &new_sh_cmd(&["gh repo view --json url"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();

    let wezterm_pane_text = String::from_utf8(
        new_sh_cmd(&[&format!(
            "wezterm cli get-text --pane-id {}",
            hx_pane.pane_id
        )])
        .output()
        .unwrap()
        .stdout,
    )
    .unwrap();

    let hx_status_line = wezterm_pane_text.lines().nth_back(1).unwrap();

    let foo = Foo::from_str(hx_status_line).unwrap();

    let path_to_github = format!("{}/{}/{}", gh_repo_view.url, current_git_branch, foo.path);

    dbg!(
        &wezterm_panes,
        &current_tab,
        &hx_pane,
        &current_git_branch,
        &gh_repo_view,
        &wezterm_pane_text,
        hx_status_line,
        &foo,
        &path_to_github
    );

    new_sh_cmd(&[&format!("echo '{}' | pbcopy", path_to_github)])
        .output()
        .unwrap();
}

fn new_sh_cmd(args: &[&str]) -> Command {
    let mut sh = Command::new("sh");
    sh.args([&["-c"], args].concat());
    sh
}

#[derive(Debug, PartialEq)]
pub struct Foo {
    path: String,
    line: i64,
    column: i64,
}

impl FromStr for Foo {
    type Err = anyhow::Error;

    fn from_str(hx_status_line: &str) -> Result<Self, Self::Err> {
        let hx_status_line = hx_status_line.trim();

        let mut elements = hx_status_line.split_ascii_whitespace();

        let path = elements.nth(2).ok_or_else(|| anyhow::anyhow!("BOOM"))?;

        let LineColumn { line, column } =
            LineColumn::from_str(elements.last().ok_or_else(|| anyhow::anyhow!("BOOM"))?)?;

        Ok(Self {
            path: path.into(),
            line,
            column,
        })
    }
}

#[derive(Debug, PartialEq)]
pub struct LineColumn {
    line: i64,
    column: i64,
}

impl FromStr for LineColumn {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (line, column) = s.split_once(':').ok_or_else(|| anyhow::anyhow!("BOOM"))?;

        Ok(Self {
            line: line.parse()?,
            column: column.parse()?,
        })
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WezTermPane {
    window_id: i64,
    tab_id: i64,
    pane_id: i64,
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

#[derive(Debug, Serialize, Deserialize)]
pub struct Size {
    rows: i64,
    cols: i64,
    pixel_width: i64,
    pixel_height: i64,
    dpi: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GhRepoView {
    url: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_foo_from_str_works_as_expected() {
        let result = Foo::from_str("● 1  bin/weh/src/main.rs                                                                      1 sel  1 char  W ● 1  42:33");
        let expected = Foo {
            path: "bin/weh/src/main.rs".into(),
            line: 42,
            column: 33,
        };

        assert_eq!(expected, result.unwrap());
    }
}
