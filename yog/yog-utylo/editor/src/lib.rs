use core::str::FromStr;
use std::path::Path;

use color_eyre::eyre;
use color_eyre::eyre::bail;
use color_eyre::eyre::eyre;
use wezterm::WeztermPane;

/// Supported text editors for file operations.
pub enum Editor {
    /// Helix editor.
    Hx,
    /// Nvim editor.
    Nvim,
}

impl Editor {
    /// Generates a command string to open the specified [`FileToOpen`] in the [`Editor`].
    pub fn open_file_cmd(&self, file_to_open: &FileToOpen) -> String {
        let path = file_to_open.path.as_str();
        let line_nbr = file_to_open.line_nbr;
        let column = file_to_open.column;

        match self {
            Self::Hx => format!("':o {path}:{line_nbr}'"),
            Self::Nvim => format!(":e {path} | :call cursor({line_nbr}, {column})"),
        }
    }

    /// Returns the pane titles associated with the [`Editor`] variant.
    pub const fn pane_titles(&self) -> &[&str] {
        match self {
            Self::Hx => &["hx"],
            Self::Nvim => &["nvim", "nv"],
        }
    }
}

/// Parses an [`Editor`] from a string representation.
impl FromStr for Editor {
    type Err = eyre::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "hx" => Ok(Self::Hx),
            "nvim" | "nv" => Ok(Self::Nvim),
            unknown => Err(eyre!("unknown editor {unknown}")),
        }
    }
}

/// Represents a file to be opened in an editor with optional line and column positioning.
#[derive(Debug, PartialEq, Eq)]
pub struct FileToOpen {
    /// The column number to position the cursor (0-based, defaults to 0).
    column: i64,
    /// The line number to position the cursor (0-based, defaults to 0).
    line_nbr: i64,
    /// The file system path to the file.
    path: String,
}

/// Attempts to create a [`FileToOpen`] from a file path, pane ID, and list of panes.
impl TryFrom<(&str, i64, &[WeztermPane])> for FileToOpen {
    type Error = eyre::Error;

    fn try_from((file_to_open, pane_id, panes): (&str, i64, &[WeztermPane])) -> Result<Self, Self::Error> {
        if Path::new(file_to_open).is_absolute() {
            return Self::from_str(file_to_open);
        }

        let mut source_pane_absolute_cwd = panes
            .iter()
            .find(|pane| pane.pane_id == pane_id)
            .ok_or_else(|| eyre!("missing panes with id {pane_id} in {panes:#?}"))?
            .absolute_cwd();

        source_pane_absolute_cwd.push(file_to_open);

        Self::from_str(
            source_pane_absolute_cwd
                .to_str()
                .ok_or_else(|| eyre!("cannot get &str from PathBuf {source_pane_absolute_cwd:#?}"))?,
        )
    }
}

/// Parses a [`FileToOpen`] from a string in the format "path:line:column".
impl FromStr for FileToOpen {
    type Err = eyre::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.split(':');
        let path = parts.next().ok_or_else(|| eyre!("missing file path found in {s}"))?;
        let line_nbr = parts.next().map(str::parse::<i64>).transpose()?.unwrap_or_default();
        let column = parts.next().map(str::parse::<i64>).transpose()?.unwrap_or_default();
        if !Path::new(path).try_exists()? {
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
    fn file_to_open_is_properly_constructed_from_expected_str() {
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
