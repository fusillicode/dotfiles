use std::path::PathBuf;
use std::process::Command;

use color_eyre::eyre::eyre;
use serde::Deserialize;

pub fn send_text_to_pane(text: &str, pane_id: i64) -> String {
    format!("wezterm cli send-text {text} --pane-id '{pane_id}' --no-paste")
}

pub fn submit_pane(pane_id: i64) -> String {
    format!(r#"printf "\r" | wezterm cli send-text --pane-id '{pane_id}' --no-paste"#)
}

pub fn activate_pane(pane_id: i64) -> String {
    format!(r#"wezterm cli activate-pane --pane-id '{pane_id}'"#)
}

pub fn get_current_pane_id() -> color_eyre::Result<i64> {
    Ok(std::env::var("WEZTERM_PANE")?.parse()?)
}

pub fn get_all_panes() -> color_eyre::Result<Vec<WeztermPane>> {
    Ok(serde_json::from_slice(
        &Command::new("wezterm")
            .args(["cli", "list", "--format", "json"])
            .output()?
            .stdout,
    )?)
}

pub fn get_sibling_pane_with_titles(
    panes: &[WeztermPane],
    current_pane_id: i64,
    pane_titles: &[&str],
) -> color_eyre::Result<WeztermPane> {
    let current_pane_tab_id = panes
        .iter()
        .find(|w| w.pane_id == current_pane_id)
        .ok_or_else(|| {
            eyre!("current pane id '{current_pane_id}' not found among panes {panes:?}")
        })?
        .tab_id;

    Ok(panes
        .iter()
        .find(|w| w.tab_id == current_pane_tab_id && pane_titles.contains(&w.title.as_str()))
        .ok_or({
            eyre!("pane with title '{pane_titles:?}' not found in tab '{current_pane_tab_id}'")
        })?
        .clone())
}

#[derive(Debug, Deserialize, Clone)]
#[cfg_attr(any(test, feature = "fake"), derive(fake::Dummy))]
pub struct WeztermPane {
    pub window_id: i64,
    pub tab_id: i64,
    pub pane_id: i64,
    pub workspace: String,
    pub size: WeztermPaneSize,
    pub title: String,
    pub cwd: PathBuf,
    pub cursor_x: i64,
    pub cursor_y: i64,
    pub cursor_shape: String,
    pub cursor_visibility: String,
    pub left_col: i64,
    pub top_row: i64,
    pub tab_title: String,
    pub window_title: String,
    pub is_active: bool,
    pub is_zoomed: bool,
    pub tty_name: String,
}

impl WeztermPane {
    pub fn absolute_cwd(&self) -> PathBuf {
        let mut path_parts = self.cwd.components();
        path_parts.next(); // Skip `file://`
        path_parts.next(); // Skip hostname

        let mut res = PathBuf::from("/");
        res.push(path_parts.collect::<PathBuf>());
        res
    }
}

#[derive(Debug, Deserialize, Clone)]
#[cfg_attr(any(test, feature = "fake"), derive(fake::Dummy))]
#[allow(dead_code)]
pub struct WeztermPaneSize {
    pub rows: i64,
    pub cols: i64,
    pub pixel_width: i64,
    pub pixel_height: i64,
    pub dpi: i64,
}
