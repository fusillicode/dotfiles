use std::process::Command;
use std::str::FromStr;

use anyhow::anyhow;

pub enum Editor {
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

    pub fn open_file_cmd(&self, file_to_open: &FileToOpen) -> String {
        let path = file_to_open.path.as_str();
        let line_nbr = file_to_open.line_nbr;

        match self {
            Self::Helix => format!("':o {path}:{line_nbr}'"),
            Self::Nvim => format!(":e {path} | :{line_nbr}"),
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

pub struct FileToOpen {
    path: String,
    line_nbr: i64,
}

impl FromStr for FileToOpen {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut file_parts = s.split(':');
        let file_path = file_parts
            .next()
            .ok_or_else(|| anyhow!("no file path found in {s}"))?;
        let line_number = file_parts
            .next()
            .map(str::parse::<i64>)
            .transpose()?
            .unwrap_or_default();

        Ok(Self {
            path: file_path.into(),
            line_nbr: line_number,
        })
    }
}

pub fn run<'a>(mut args: impl Iterator<Item = &'a str>) -> anyhow::Result<()> {
    let Some(editor) = args.next().map(Editor::from_str).transpose()? else {
        return Err(anyhow!(
            "no editor specified {:?}",
            args.collect::<Vec<_>>()
        ));
    };

    let Some(file_to_open) = args.next().map(FileToOpen::from_str).transpose()? else {
        return Err(anyhow!(
            "no input file specified {:?}",
            args.collect::<Vec<_>>()
        ));
    };

    let editor_pane_id =
        crate::utils::wezterm::get_current_pane_sibling_with_one_of_titles(editor.pane_titles())
            .map(|x| x.pane_id)?;

    let open_file_cmd = editor.open_file_cmd(&file_to_open);

    Command::new("sh")
        .args([
            "-c",
            &format!(
                r#"
                    wezterm cli send-text --pane-id '{editor_pane_id}' '{open_file_cmd}' --no-paste && \
                        printf "\r" | wezterm cli send-text --pane-id '{editor_pane_id}' --no-paste && \
                        wezterm cli activate-pane --pane-id '{editor_pane_id}'
                "#,
            ),
        ])
        .spawn()?;

    Ok(())
}
