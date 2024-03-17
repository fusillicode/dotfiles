use std::path::PathBuf;
use std::process::Command;

use anyhow::anyhow;
use serde::Deserialize;

pub fn get_current_pane_sibling_with_one_of_titles(
    pane_titles: &[&str],
) -> anyhow::Result<WezTermPane> {
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
        .find(|w| w.tab_id == current_pane_tab_id && pane_titles.contains(&w.title.as_str()))
        .ok_or({
            anyhow!("pane with title '{pane_titles:?}' not found in tab '{current_pane_tab_id}'")
        })?
        .clone())
}

#[derive(Debug, Deserialize, Clone)]
#[cfg_attr(any(test), derive(fake::Dummy))]
#[allow(dead_code)]
pub struct WezTermPane {
    pub window_id: i64,
    pub tab_id: i64,
    pub pane_id: i64,
    pub workspace: String,
    pub size: WezTermPaneSize,
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

#[derive(Debug, Deserialize, Clone)]
#[cfg_attr(any(test), derive(fake::Dummy))]
#[allow(dead_code)]
pub struct WezTermPaneSize {
    pub rows: i64,
    pub cols: i64,
    pub pixel_width: i64,
    pub pixel_height: i64,
    pub dpi: i64,
}
