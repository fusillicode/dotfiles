use std::path::Path;
use std::str::FromStr;

use anyhow::anyhow;
use anyhow::bail;

use crate::utils::system::silent_cmd;
use crate::utils::wezterm::WezTermPane;

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
        let column = file_to_open.column;

        match self {
            Self::Helix => format!("':o {path}:{line_nbr}'"),
            Self::Nvim => format!(":e {path} | :call cursor({line_nbr}, {column})"),
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

#[derive(Debug, PartialEq)]
pub struct FileToOpen {
    path: String,
    line_nbr: i64,
    column: i64,
}

impl TryFrom<(&str, i64, &[WezTermPane])> for FileToOpen {
    type Error = anyhow::Error;

    fn try_from(
        (file_to_open, pane_id, panes): (&str, i64, &[WezTermPane]),
    ) -> Result<Self, Self::Error> {
        if Path::new(file_to_open).is_absolute() {
            return Self::from_str(file_to_open);
        }

        let mut source_pane_absolute_cwd = panes
            .iter()
            .find(|p| p.pane_id == pane_id)
            .ok_or_else(|| anyhow!("no panes matching id {pane_id} in {panes:?}"))?
            .absolute_cwd();

        source_pane_absolute_cwd.push(file_to_open);

        Self::from_str(
            source_pane_absolute_cwd.to_str().ok_or_else(|| {
                anyhow!("cannot get &str from PathBuf {source_pane_absolute_cwd:?}")
            })?,
        )
    }
}

impl FromStr for FileToOpen {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.split(':');
        let path = parts
            .next()
            .ok_or_else(|| anyhow!("no file path found in {s}"))?;
        let line_nbr = parts
            .next()
            .map(str::parse::<i64>)
            .transpose()?
            .unwrap_or_default();
        let column = parts
            .next()
            .map(str::parse::<i64>)
            .transpose()?
            .unwrap_or_default();
        if !Path::new(path).exists() {
            bail!("file {path} doesn't exists")
        }

        Ok(Self {
            path: path.into(),
            line_nbr,
            column,
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

    let panes = crate::utils::wezterm::get_all_panes()?;

    let file_to_open = FileToOpen::try_from((file_to_open, pane_id, panes.as_slice()))?;

    let editor_pane_id = crate::utils::wezterm::get_sibling_pane_matching_titles(
        &panes,
        pane_id,
        editor.pane_titles(),
    )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_to_open_is_properly_constructed_from_expected_str() {
        assert_eq!(
            FileToOpen {
                path: "bootstrap.sh".into(),
                line_nbr: 0,
                column: 0
            },
            FileToOpen::from_str("bootstrap.sh").unwrap()
        );
        assert_eq!(
            FileToOpen {
                path: "bootstrap.sh".into(),
                line_nbr: 3,
                column: 0
            },
            FileToOpen::from_str("bootstrap.sh:3").unwrap()
        );
        assert_eq!(
            FileToOpen {
                path: "bootstrap.sh".into(),
                line_nbr: 3,
                column: 7
            },
            FileToOpen::from_str("bootstrap.sh:3:7").unwrap()
        );
        assert_eq!(
            FileToOpen {
                path: ".bootstrap.sh".into(),
                line_nbr: 0,
                column: 0
            },
            FileToOpen::from_str(".bootstrap.sh").unwrap()
        );
        assert_eq!(
            FileToOpen {
                path: ".bootstrap.sh".into(),
                line_nbr: 3,
                column: 0
            },
            FileToOpen::from_str(".bootstrap.sh:3").unwrap()
        );
        assert_eq!(
            FileToOpen {
                path: ".bootstrap.sh".into(),
                line_nbr: 3,
                column: 7
            },
            FileToOpen::from_str(".bootstrap.sh:3:7").unwrap()
        );
        assert_eq!(
            FileToOpen {
                path: "/root/bootstrap.sh".into(),
                line_nbr: 0,
                column: 0
            },
            FileToOpen::from_str("/root/bootstrap.sh").unwrap()
        );
        assert_eq!(
            FileToOpen {
                path: "/root/bootstrap.sh".into(),
                line_nbr: 3,
                column: 0
            },
            FileToOpen::from_str("/root/bootstrap.sh:3").unwrap()
        );
        assert_eq!(
            FileToOpen {
                path: "/root/bootstrap.sh".into(),
                line_nbr: 3,
                column: 7
            },
            FileToOpen::from_str("/root/bootstrap.sh:3:7").unwrap()
        );
    }
}
