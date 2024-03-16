use std::process::Command;
use std::str::FromStr;

use anyhow::anyhow;

enum Editor {
    Helix,
    Nvim,
}

impl Editor {
    pub fn pane_titles(&self) -> &[&str] {
        match self {
            Self::Helix => &["hx"],
            Self::Nvim => &["nvim", "nv"],
        }
    }
}

impl FromStr for Editor {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "hx" => Ok(Self::Helix),
            "nvim" | "nv" => Ok(Self::Nvim),
            s => Err(anyhow!("unknown editor {s}")),
        }
    }
}

pub fn run<'a>(mut args: impl Iterator<Item = &'a str>) -> anyhow::Result<()> {
    let Some(editor) = args.next().map(Editor::from_str).transpose()? else {
        return Err(anyhow!(
            "no editor specified {:?}",
            args.collect::<Vec<_>>()
        ));
    };

    let file_to_open = args
        .next()
        .ok_or_else(|| anyhow!("no input file specified {:?}", args.collect::<Vec<_>>()))?;

    let editor_pane_id = {
        let mut errors = vec![];
        'editor_pane_id: for editor_pane_title in editor.pane_titles() {
            match crate::utils::wezterm::get_current_pane_sibling_with_title(editor_pane_title)
                .map(|x| x.pane_id)
            {
                Ok(_) => break 'editor_pane_id,
                Err(error) => errors.push(error),
            }
        }
        Result::<i64, _>::Err(anyhow!("foo {errors:?}"))
    }?;

    Command::new("sh")
        .args([
            "-c",
            &format!(
                r#"
                    wezterm cli send-text --pane-id '{editor_pane_id}' ':o {file_to_open}' --no-paste && \
                        printf "\r" | wezterm cli send-text --pane-id '{editor_pane_id}' --no-paste && \
                        wezterm cli activate-pane --pane-id '{editor_pane_id}'
                "#,
            ),
        ])
        .spawn()?;

    Ok(())
}
