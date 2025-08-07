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

    load_additional_paths()?;
    let args = utils::system::get_args();

    let Some(editor) = args.first().map(|x| Editor::from_str(x)).transpose()? else {
        bail!("no editor specified {args:?}");
    };

    let Some(file_to_open) = args.get(1) else {
        bail!("no input file specified {args:?}");
    };

    let pane_id = match args.get(2) {
        Some(x) => x.parse()?,
        None => utils::wezterm::get_current_pane_id()?,
    };

    let panes = utils::wezterm::get_all_panes()?;

    let file_to_open = FileToOpen::try_from((file_to_open.as_str(), pane_id, panes.as_slice()))?;

    let editor_pane_id =
        utils::wezterm::get_sibling_pane_with_titles(&panes, pane_id, editor.pane_titles())
            .map(|x| x.pane_id)?;

    let open_file_cmd = editor.open_file_cmd(&file_to_open);

    utils::cmd::silent_cmd("sh")
        .args([
            "-c",
            &format!(
                // `wezterm cli send-text $'\e'` sends the "ESC" to Wezterm to exit from insert mode
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
fn load_additional_paths() -> color_eyre::Result<()> {
    let home = std::env::var("HOME")?;

    let new_path = [
        &std::env::var("PATH").unwrap_or_else(|_| String::new()),
        "/opt/homebrew/bin",
        &format!("{home}/.local/bin"),
    ]
    .join(":");

    unsafe {
        std::env::set_var("PATH", &new_path);
    }
    Ok(())
}
