use std::str::FromStr;

use anyhow::anyhow;

use utils::editor::Editor;
use utils::editor::FileToOpen;
use utils::system::silent_cmd;

/// Open the supplied file path in a running editor (Neovim or Helix) instance alongside the
/// Wezterm pane from where the cmd has been invoked.
/// Used both as a CLI and from Wezterm `open-uri` handler.
pub fn run<'a>(mut args: impl Iterator<Item = &'a str>) -> anyhow::Result<()> {
    let Some(editor) = args.next().map(Editor::from_str).transpose()? else {
        return Err(anyhow!(
            "no editor specified {:?}",
            args.collect::<Vec<_>>()
        ));
    };

    let Some(file_to_open) = args.next() else {
        return Err(anyhow!(
            "no input file specified {:?}",
            args.collect::<Vec<_>>()
        ));
    };

    let pane_id = match args.next() {
        Some(x) => x.into(),
        None => std::env::var("WEZTERM_PANE")?,
    }
    .parse()?;

    let panes = utils::wezterm::get_all_panes()?;

    let file_to_open = FileToOpen::try_from((file_to_open, pane_id, panes.as_slice()))?;

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
