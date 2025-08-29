use std::path::PathBuf;
use std::process::Command;

use color_eyre::eyre::eyre;
use serde::Deserialize;

/// Generates a command string to send text to a specific [`WeztermPane`] using the Wezterm CLI.
pub fn send_text_to_pane_cmd(text: &str, pane_id: i64) -> String {
    format!("wezterm cli send-text {text} --pane-id '{pane_id}' --no-paste")
}

/// Generates a command string to submit (send a carriage return) to a specific [`WeztermPane`].
pub fn submit_pane_cmd(pane_id: i64) -> String {
    format!(r#"printf "\r" | wezterm cli send-text --pane-id '{pane_id}' --no-paste"#)
}

/// Generates a command string to activate a specific [`WeztermPane`] using the Wezterm CLI.
pub fn activate_pane_cmd(pane_id: i64) -> String {
    format!("wezterm cli activate-pane --pane-id '{pane_id}'")
}

/// Retrieves the current pane ID from the `WEZTERM_PANE` environment variable.
///
/// # Errors
///
/// Returns an error if the environment variable is missing or not an integer.
pub fn get_current_pane_id() -> color_eyre::Result<i64> {
    Ok(std::env::var("WEZTERM_PANE")?.parse()?)
}

/// Retrieves all Wezterm panes using the Wezterm CLI.
///
/// The `envs` parameter is required because Wezterm may not be found in the PATH
/// when called by the `oe` CLI when a file path is clicked in Wezterm itself.
///
/// # Errors
///
/// Returns an error if invoking `wezterm` fails or the JSON cannot be parsed.
pub fn get_all_panes(envs: &[(&str, &str)]) -> color_eyre::Result<Vec<WeztermPane>> {
    let mut cmd = Command::new("wezterm");
    cmd.args(["cli", "list", "--format", "json"]);
    cmd.envs(envs.iter().copied());
    Ok(serde_json::from_slice(&cmd.output()?.stdout)?)
}

/// Finds a sibling [`WeztermPane`] in the same tab that matches one of the given titles.
///
/// # Errors
///
/// Returns an error if the current pane ID is not found, or if no matching pane exists in the tab.
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
        .ok_or_else(|| eyre!("pane with title '{pane_titles:#?}' not found in tab '{current_pane_tab_id}'"))?
        .clone())
}

/// Represents a Wezterm pane with all its properties and state information.
#[derive(Debug, Deserialize, Clone)]
#[cfg_attr(any(test, feature = "fake"), derive(fake::Dummy))]
pub struct WeztermPane {
    /// The shape of the cursor.
    pub cursor_shape: String,
    /// The visibility state of the cursor.
    pub cursor_visibility: String,
    /// The X coordinate of the cursor.
    pub cursor_x: i64,
    /// The Y coordinate of the cursor.
    pub cursor_y: i64,
    /// The current working directory as a file URI.
    pub cwd: PathBuf,
    /// Whether this pane is currently active.
    pub is_active: bool,
    /// Whether this pane is zoomed (maximized).
    pub is_zoomed: bool,
    /// The left column position of the pane.
    pub left_col: i64,
    /// The unique ID of this pane.
    pub pane_id: i64,
    /// The size dimensions of the pane.
    pub size: WeztermPaneSize,
    /// The ID of the tab containing this pane.
    pub tab_id: i64,
    /// The title of the tab containing this pane.
    pub tab_title: String,
    /// The title of the pane.
    pub title: String,
    /// The top row position of the pane.
    pub top_row: i64,
    /// The TTY device name associated with this pane.
    pub tty_name: String,
    /// The ID of the window containing this pane.
    pub window_id: i64,
    /// The title of the window containing this pane.
    pub window_title: String,
    /// The workspace name.
    pub workspace: String,
}

impl WeztermPane {
    /// Given two [`WeztermPane`] checks if they are in the same tab and if the first
    /// has a current working directory that is the same or a child of the second one.
    pub fn is_sibling_terminal_pane_of(&self, other: &WeztermPane) -> bool {
        self.pane_id != other.pane_id && self.tab_id == other.tab_id && self.cwd.starts_with(&other.cwd)
    }

    /// Converts the current working directory from a file URI to an absolute [`PathBuf`].
    pub fn absolute_cwd(&self) -> PathBuf {
        let mut path_parts = self.cwd.components();
        path_parts.next(); // Skip `file://`
        path_parts.next(); // Skip hostname
        PathBuf::from("/").join(path_parts.collect::<PathBuf>())
    }
}

/// Represents the size and dimensions of a Wezterm pane.
#[derive(Debug, Deserialize, Clone)]
#[cfg_attr(any(test, feature = "fake"), derive(fake::Dummy))]
pub struct WeztermPaneSize {
    /// Number of character columns in the pane.
    pub cols: i64,
    /// Dots per inch (DPI) of the display.
    pub dpi: i64,
    /// Height of the pane in pixels.
    pub pixel_height: i64,
    /// Width of the pane in pixels.
    pub pixel_width: i64,
    /// Number of character rows in the pane.
    pub rows: i64,
}
