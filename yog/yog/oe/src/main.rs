//! Open files (optionally at line:col) in existing Neovim / Helix pane.
//!
//! # Arguments
//! - `editor` Target editor (`nvim` | `hx`).
//! - `file_path` File to open (append :line:col to jump location).
//! - `pane_id` Optional WezTerm pane ID (auto-detected if omitted).
//!
//! # Errors
//! - Missing editor or file argument.
//! - Pane id parse or discovery fails.
//! - Sibling pane detection fails.
//! - File path parsing / validation fails.
//! - Spawning shell command fails.
//! - Required environment variable read fails.
#![feature(exit_status_error)]

use core::str::FromStr;

use color_eyre::eyre::bail;
use ytil_editor::Editor;
use ytil_editor::FileToOpen;

/// Open files in a running editor instance from `WezTerm`.
///
/// # Usage
/// ```bash
/// oe nvim path/to/file.rs # open file in existing nvim pane
/// oe hx path/to/file.rs:42:5 # open file at line 42 col 5 in helix
/// oe nvim src/lib.rs 123456789 # explicitly pass pane id
/// ```
///
/// # Arguments
/// - `editor` Editor to use ("nvim" or "hx").
/// - `file_path` Path to file to open (supports :line:col suffix).
/// - `pane_id` Optional `WezTerm` pane ID (auto-detected if omitted).
///
/// # Errors
/// - Editor argument is missing.
/// - File path argument is missing.
/// - Pane id argument is invalid (parse failure) or current pane lookup fails.
/// - `WezTerm` Pane enumeration / sibling lookup fails.
/// - File path parsing / validation for the target editor fails.
/// - Spawning the shell command to drive `WezTerm` fails.
fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let enriched_path_env = get_enriched_path_env()?;
    let args = ytil_system::get_args();

    let Some(editor) = args.first().map(|x| Editor::from_str(x)).transpose()? else {
        bail!("missing editor arg | args={args:#?}");
    };

    let Some(file_to_open) = args.get(1) else {
        bail!("missing file arg | args={args:#?}");
    };

    let pane_id = match args.get(2) {
        Some(x) => x.parse()?,
        None => ytil_wezterm::get_current_pane_id()?,
    };

    let panes = ytil_wezterm::get_all_panes(&[enriched_path_env.by_ref()])?;

    let file_to_open = FileToOpen::try_from((file_to_open.as_str(), pane_id, panes.as_slice()))?;

    let editor_pane_id =
        ytil_wezterm::get_sibling_pane_with_titles(&panes, pane_id, editor.pane_titles()).map(|x| x.pane_id)?;

    let open_file_cmd = editor.open_file_cmd(&file_to_open);

    ytil_cmd::silent_cmd("sh")
        .args([
            "-c",
            &format!(
                "{} && {} && {} && {}",
                // `wezterm cli send-text $'\e'` sends the "ESC" to `WezTerm` to exit from insert mode
                // https://github.com/wez/wezterm/discussions/3945
                ytil_wezterm::send_text_to_pane_cmd(r"$'\e'", editor_pane_id),
                ytil_wezterm::send_text_to_pane_cmd(&format!("'{open_file_cmd}'"), editor_pane_id),
                ytil_wezterm::submit_pane_cmd(editor_pane_id),
                ytil_wezterm::activate_pane_cmd(editor_pane_id),
            ),
        ])
        .envs(std::iter::once(enriched_path_env.by_ref()))
        .spawn()?;

    Ok(())
}

/// Creates enriched PATH for `WezTerm` integration.
///
/// # Errors
/// - A required environment variable is missing or invalid Unicode.
fn get_enriched_path_env() -> color_eyre::Result<Env> {
    let enriched_path = [
        &std::env::var("PATH").unwrap_or_else(|_| String::new()),
        "/opt/homebrew/bin",
        &ytil_system::build_home_path(&[".local", "bin"])?.to_string_lossy(),
    ]
    .join(":");

    Ok(Env {
        name: "PATH",
        value: enriched_path,
    })
}

/// Wrapper for environment variables.
struct Env {
    /// The name of the environment variable (static string).
    name: &'static str,
    /// The value of the environment variable (dynamically constructed string).
    value: String,
}

impl Env {
    /// Returns environment variable as tuple.
    pub fn by_ref(&self) -> (&'static str, &str) {
        (self.name, &self.value)
    }
}
