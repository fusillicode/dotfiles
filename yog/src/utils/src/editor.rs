use std::path::Path;
use std::str::FromStr;

use anyhow::anyhow;
use anyhow::bail;

use crate::wezterm::WezTermPane;

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
            .ok_or_else(|| anyhow!("no panes with id {pane_id} in {panes:?}"))?
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_to_open_is_properly_constructed_from_expected_str() {
        let root_dir = std::env::current_dir().unwrap();
        // We should always have a Cargo.toml...
        let dummy_path = root_dir.join("Cargo.toml").to_string_lossy().to_string();

        assert_eq!(
            FileToOpen {
                path: dummy_path.clone(),
                line_nbr: 0,
                column: 0
            },
            FileToOpen::from_str(&dummy_path).unwrap()
        );
        assert_eq!(
            FileToOpen {
                path: dummy_path.clone(),
                line_nbr: 3,
                column: 0
            },
            FileToOpen::from_str(&format!("{dummy_path}:3")).unwrap()
        );
        assert_eq!(
            FileToOpen {
                path: dummy_path.clone(),
                line_nbr: 3,
                column: 7
            },
            FileToOpen::from_str(&format!("{dummy_path}:3:7")).unwrap()
        );
    }
}
