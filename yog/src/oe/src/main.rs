#![feature(exit_status_error)]

use core::str::FromStr;

use color_eyre::eyre::bail;
use utils::editor::Editor;
use utils::editor::FileToOpen;

/// Opens files in running editor instance from Wezterm.
///
/// # Arguments
///
/// * `editor` - Editor to use ("nvim" or "hx")
/// * `file_path` - Path to file to open
/// * `pane_id` - Optional Wezterm pane ID
///
/// # Examples
///
/// ```bash
/// oe nvim /path/to/file.rs
/// oe hx /path/to/file.rs
/// oe nvim /path/to/file.rs 5
/// ```
fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let enriched_path_env = get_enriched_path_env()?;
    let args = utils::system::get_args();

    let Some(editor) = args.first().map(|x| Editor::from_str(x)).transpose()? else {
        bail!("missing editor specified {args:#?}");
    };

    let Some(file_to_open) = args.get(1) else {
        bail!("missing input file specified {args:#?}");
    };

    let pane_id = match args.get(2) {
        Some(x) => x.parse()?,
        None => utils::wezterm::get_current_pane_id()?,
    };

    let panes = utils::wezterm::get_all_panes(&[enriched_path_env.by_ref()])?;

    let file_to_open = FileToOpen::try_from((file_to_open.as_str(), pane_id, panes.as_slice()))?;

    let editor_pane_id =
        utils::wezterm::get_sibling_pane_with_titles(&panes, pane_id, editor.pane_titles()).map(|x| x.pane_id)?;

    let open_file_cmd = editor.open_file_cmd(&file_to_open);

    utils::cmd::silent_cmd("sh")
        .args([
            "-c",
            &format!(
                "{} && {} && {} && {}",
                // `wezterm cli send-text $'\e'` sends the "ESC" to Wezterm to exit from insert mode
                // https://github.com/wez/wezterm/discussions/3945
                utils::wezterm::send_text_to_pane_cmd(r#"$'\e'"#, editor_pane_id),
                utils::wezterm::send_text_to_pane_cmd(&format!("'{open_file_cmd}'"), editor_pane_id),
                utils::wezterm::submit_pane_cmd(editor_pane_id),
                utils::wezterm::activate_pane_cmd(editor_pane_id),
            ),
        ])
        .envs([enriched_path_env.by_ref()].iter().copied())
        .spawn()?;

    Ok(())
}

/// Creates enriched PATH for Wezterm integration.
fn get_enriched_path_env() -> color_eyre::Result<Env> {
    let enriched_path = [
        &std::env::var("PATH").unwrap_or_else(|()| String::new()),
        "/opt/homebrew/bin",
        &utils::system::build_home_path(&[".local", "bin"])?.to_string_lossy(),
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
