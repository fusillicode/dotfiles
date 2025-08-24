use std::path::PathBuf;
use std::process::Command;

use color_eyre::eyre::eyre;
use serde::Deserialize;

/// Generates a command string to send text to a specific [WeztermPane] using the Wezterm CLI.
pub fn send_text_to_pane_cmd(text: &str, pane_id: i64) -> String {
    format!("wezterm cli send-text {text} --pane-id '{pane_id}' --no-paste")
}

/// Generates a command string to submit (send a carriage return) to a specific [WeztermPane].
pub fn submit_pane_cmd(pane_id: i64) -> String {
    format!(r#"printf "\r" | wezterm cli send-text --pane-id '{pane_id}' --no-paste"#)
}

/// Generates a command string to activate a specific [WeztermPane] using the Wezterm CLI.
pub fn activate_pane_cmd(pane_id: i64) -> String {
    format!(r#"wezterm cli activate-pane --pane-id '{pane_id}'"#)
}

/// Retrieves the current pane ID from the WEZTERM_PANE environment variable.
pub fn get_current_pane_id() -> color_eyre::Result<i64> {
    Ok(std::env::var("WEZTERM_PANE")?.parse()?)
}

// envs is required because Wezterm is not found when called by `oe` CLI when a file path is
// clicked in Wezterm itself.
pub fn get_all_panes(envs: &[(&str, &str)]) -> color_eyre::Result<Vec<WeztermPane>> {
    let mut cmd = Command::new("wezterm");
    cmd.args(["cli", "list", "--format", "json"]);
    cmd.envs(envs.iter().copied());
    Ok(serde_json::from_slice(&cmd.output()?.stdout)?)
}

/// Finds a sibling [WeztermPane] in the same tab that matches one of the given titles.
pub fn get_sibling_pane_with_titles(
    panes: &[WeztermPane],
    current_pane_id: i64,
    pane_titles: &[&str],
) -> color_eyre::Result<WeztermPane> {
    let current_pane_tab_id = panes
        .iter()
        .find(|w| w.pane_id == current_pane_id)
        .ok_or_else(|| eyre!("current pane id '{current_pane_id}' not found among panes {panes:#?}"))?
        .tab_id;

    Ok(panes
        .iter()
        .find(|w| w.tab_id == current_pane_tab_id && pane_titles.contains(&w.title.as_str()))
        .ok_or(eyre!(
            "pane with title '{pane_titles:#?}' not found in tab '{current_pane_tab_id}'"
        ))?
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
    /// Given two [WeztermPane] checks if they are in the same tab and if the first
    /// has a current working directory that is the same or a child of the second one.
    pub fn is_sibling_terminal_pane_of(&self, other: &WeztermPane) -> bool {
        self.pane_id != other.pane_id && self.tab_id == other.tab_id && self.cwd.starts_with(&other.cwd)
    }
}

impl WeztermPane {
    /// Converts the current working directory from a file URI to an absolute [PathBuf].
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
