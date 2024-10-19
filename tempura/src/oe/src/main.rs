#![feature(exit_status_error)]
use std::str::FromStr;

use anyhow::bail;

use utils::editor::Editor;
use utils::editor::FileToOpen;
use utils::system::silent_cmd;

/// Open the supplied file path in a running editor (Neovim or Helix) instance alongside the
/// Wezterm pane from where the cmd has been invoked.
/// Used both as a CLI and from Wezterm `open-uri` handler.
fn main() -> anyhow::Result<()> {
    load_additional_paths()?;
    let args = utils::system::get_args();

    let Some(editor) = args.first().map(|x| Editor::from_str(x)).transpose()? else {
        bail!("no editor specified {:?}", args);
    };

    let Some(file_to_open) = args.get(1) else {
        bail!("no input file specified {:?}", args);
    };

    let pane_id = match args.get(2) {
        Some(x) => x.into(),
        None => std::env::var("WEZTERM_PANE")?,
    }
    .parse()?;

    let panes = utils::wezterm::get_all_panes()?;

    let file_to_open = FileToOpen::try_from((file_to_open.as_str(), pane_id, panes.as_slice()))?;

    let editor_pane_id =
        utils::wezterm::get_sibling_pane_with_titles(&panes, pane_id, editor.pane_titles())
            .map(|x| x.pane_id)?;

    let open_file_cmd = editor.open_file_cmd(&file_to_open);

    silent_cmd("sh")
        .args([
            "-c",
            &format!(
                // `wezterm cli send-text $'\e'` sends the "ESC" to WezTerm to exit from insert mode
                // https://github.com/wez/wezterm/discussions/3945
                r#"
                    wezterm cli send-text $'\e' --pane-id '{editor_pane_id}' --no-paste && \
                        wezterm cli send-text '{open_file_cmd}' --pane-id '{editor_pane_id}' --no-paste && \
                        printf "\r" | wezterm cli send-text --pane-id '{editor_pane_id}' --no-paste && \
                        wezterm cli activate-pane --pane-id '{editor_pane_id}'
                "#,
            ),
        ])
        .spawn()?;

    Ok(())
}

// Needed because calling oe from wezterm open-uri handler doesn't retain the PATH
pub fn load_additional_paths() -> anyhow::Result<()> {
    let home = std::env::var("HOME")?;

    let new_path = [
        &std::env::var("PATH").unwrap_or_else(|_| String::new()),
        "/opt/homebrew/bin",
        &format!("{home}/.local/bin"),
    ]
    .join(":");

    std::env::set_var("PATH", &new_path);
    Ok(())
}
