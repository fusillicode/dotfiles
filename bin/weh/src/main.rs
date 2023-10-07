use std::process::Command;
use std::str::FromStr;

use serde::Deserialize;
use url::Url;

fn main() -> anyhow::Result<()> {
    let args = std::env::args().collect::<Vec<String>>();
    let (_, args) = args.split_first().unwrap();

    let (cmd, args) = args
        .split_first()
        .map(|(cmd, rest)| (cmd.as_str(), rest.iter().map(String::as_str)))
        .unwrap();

    match cmd {
        "gh" => copy_link_to_github(args),
        "ho" => open_in_hx(args),
        unexpected_cmd => anyhow::bail!("BOOM {} {:?}", unexpected_cmd, args.collect::<Vec<_>>()),
    }
}

fn open_in_hx<'a>(mut args: impl Iterator<Item = &'a str>) -> Result<(), anyhow::Error> {
    let Some(file_to_open) = args.next() else {
        anyhow::bail!("BOOM")
    };

    let hx_pane_id = 2;

    new_sh_cmd(&format!(
        r#"
            wezterm cli send-text --pane-id '{hx_pane_id}' ':o {file_to_open}' --no-paste && \
                printf "\r" | wezterm cli send-text --pane-id '{hx_pane_id}' --no-paste && \
                wezterm cli activate-pane --pane-id '{hx_pane_id}'
        "#,
    ))
    .spawn()
    .unwrap();

    Ok(())
}

fn copy_link_to_github<'a>(_args: impl Iterator<Item = &'a str>) -> Result<(), anyhow::Error> {
    let wezterm_panes: Vec<WezTermPane> = serde_json::from_slice(
        &new_sh_cmd("wezterm cli list --format json")
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
        new_sh_cmd("git branch --show-current")
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    let current_git_branch = current_git_branch.trim();

    let gh_repo_view: GhRepoView = serde_json::from_slice(
        &new_sh_cmd("gh repo view --json url")
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();

    let wezterm_pane_text = String::from_utf8(
        new_sh_cmd(&format!(
            "wezterm cli get-text --pane-id {}",
            hx_pane.pane_id
        ))
        .output()
        .unwrap()
        .stdout,
    )
    .unwrap();

    let hx_status_line = wezterm_pane_text.lines().nth_back(1).unwrap();

    let hx_position = HxPosition::from_str(hx_status_line).unwrap();

    let mut link_to_github = gh_repo_view.url;
    link_to_github.set_path(&format!(
        "tree/{}/{}",
        current_git_branch, hx_position.file_path
    ));
    link_to_github.set_fragment(Some(&format!(
        "L{}C{}",
        hx_position.line, hx_position.column
    )));

    new_sh_cmd(&format!("echo '{}' | pbcopy", link_to_github))
        .output()
        .unwrap();

    Ok(())
}

fn new_sh_cmd(cmd: &str) -> Command {
    let mut sh = Command::new("sh");
    sh.args(["-c", cmd]);
    sh
}

#[derive(Debug, PartialEq)]
pub struct HxPosition {
    file_path: String,
    line: i64,
    column: i64,
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

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
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

#[derive(Debug, Deserialize)]
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
    url: Url,
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
