#![feature(exit_status_error)]

use std::str::FromStr;

use color_eyre::eyre::bail;

use utils::editor::Editor;
use utils::editor::FileToOpen;

/// Open the supplied file path in a running editor (Neovim or Helix) instance alongside the
/// Wezterm pane from where the cmd has been invoked.
/// Used both as a CLI and from Wezterm `open-uri` handler.
fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let enriched_path_env = get_enriched_path_env()?;
    let args = utils::system::get_args();

    let Some(editor) = args.first().map(|x| Editor::from_str(x)).transpose()? else {
        bail!("no editor specified {args:#?}");
    };

    let Some(file_to_open) = args.get(1) else {
        bail!("no input file specified {args:#?}");
    };

    let pane_id = match args.get(2) {
        Some(x) => x.parse()?,
        None => utils::wezterm::get_current_pane_id()?,
    };

    let panes = utils::wezterm::get_all_panes(&[enriched_path_env.by_ref()])?;

    let file_to_open = FileToOpen::try_from((file_to_open.as_str(), pane_id, panes.as_slice()))?;

    let editor_pane_id =
        utils::wezterm::get_sibling_pane_with_titles(&panes, pane_id, editor.pane_titles())
            .map(|x| x.pane_id)?;

    let open_file_cmd = editor.open_file_cmd(&file_to_open);

    utils::cmd::silent_cmd("sh")
        .args([
            "-c",
            &format!(
                "{} && {} && {} && {}",
                // `wezterm cli send-text $'\e'` sends the "ESC" to Wezterm to exit from insert mode
                // https://github.com/wez/wezterm/discussions/3945
                utils::wezterm::send_text_to_pane(r#"$'\e'"#, editor_pane_id),
                utils::wezterm::send_text_to_pane(&format!("'{open_file_cmd}'"), editor_pane_id),
                utils::wezterm::submit_pane(editor_pane_id),
                utils::wezterm::activate_pane(editor_pane_id),
            ),
        ])
        .envs([enriched_path_env.by_ref()].iter().copied())
        .spawn()?;

    Ok(())
}

// Needed because calling `oe` from Wezterm open-uri handler doesn't retain the PATH
fn get_enriched_path_env() -> color_eyre::Result<Env> {
    let enriched_path = [
        &std::env::var("PATH").unwrap_or_else(|_| String::new()),
        "/opt/homebrew/bin",
        &format!("{}/.local/bin", std::env::var("HOME")?),
    ]
    .join(":");
    Ok(Env("PATH".into(), enriched_path))
}

/// New type to get a [`(&str, &str)`] from a [`(String, String)`].
struct Env(String, String);

impl Env {
    pub fn by_ref(&self) -> (&str, &str) {
        (&self.0, &self.1)
    }
}
