//! Parse `path:line[:column]` specs and build editor open commands for Helix / Neovim panes.
//!
//! Supports absolute or relative paths (resolved against a pane's cwd) and returns shell snippets
//! to open a file and place the cursor at the requested position.

use core::str::FromStr;
use std::path::Path;

use color_eyre::eyre::bail;
use color_eyre::eyre::eyre;
use ytil_wezterm::WeztermPane;

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
    type Err = color_eyre::eyre::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "hx" => Ok(Self::Hx),
            "nvim" | "nv" => Ok(Self::Nvim),
            unknown => Err(eyre!("unknown editor | value={unknown}")),
        }
    }
}

/// Represents a file to be opened in an editor with optional line and column positioning.
#[derive(Debug, Eq, PartialEq)]
pub struct FileToOpen {
    /// The column number to position the cursor (0-based, defaults to 0).
    column: i64,
    /// The line number to position the cursor (0-based, defaults to 0).
    line_nbr: i64,
    /// The filesystem path to the file.
    path: String,
}

/// Attempts to create a [`FileToOpen`] from a file path, pane ID, and list of panes.
impl TryFrom<(&str, i64, &[WeztermPane])> for FileToOpen {
    type Error = color_eyre::eyre::Error;

    fn try_from((file_to_open, pane_id, panes): (&str, i64, &[WeztermPane])) -> Result<Self, Self::Error> {
        if Path::new(file_to_open).is_absolute() {
            return Self::from_str(file_to_open);
        }

        let mut source_pane_absolute_cwd = panes
            .iter()
            .find(|pane| pane.pane_id == pane_id)
            .ok_or_else(|| eyre!("missing pane | pane_id={pane_id} panes={panes:#?}"))?
            .absolute_cwd();

        source_pane_absolute_cwd.push(file_to_open);

        Self::from_str(
            source_pane_absolute_cwd
                .to_str()
                .ok_or_else(|| eyre!("cannot get path str | path={source_pane_absolute_cwd:#?}"))?,
        )
    }
}

/// Parses a [`FileToOpen`] from a string in the format "path:line:column".
impl FromStr for FileToOpen {
    type Err = color_eyre::eyre::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.split(':');
        let path = parts.next().ok_or_else(|| eyre!("file path missing | str={s}"))?;
        let line_nbr = parts.next().map(str::parse::<i64>).transpose()?.unwrap_or_default();
        let column = parts.next().map(str::parse::<i64>).transpose()?.unwrap_or_default();
        if !Path::new(path).try_exists()? {
            bail!("file missing | path={path}")
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
    use std::path::PathBuf;

    use ytil_wezterm::WeztermPane;
    use ytil_wezterm::WeztermPaneSize;

    use super::*;

    #[test]
    fn open_file_cmd_returns_the_expected_cmd_string() {
        let file = FileToOpen {
            path: "src/main.rs".into(),
            line_nbr: 12,
            column: 5,
        };
        assert_eq!(Editor::Hx.open_file_cmd(&file), "':o src/main.rs:12'");
        assert_eq!(
            Editor::Nvim.open_file_cmd(&file),
            ":e src/main.rs | :call cursor(12, 5)"
        );
    }

    #[test]
    fn pane_titles_are_the_expected_ones() {
        assert_eq!(Editor::Hx.pane_titles(), &["hx"]);
        assert_eq!(Editor::Nvim.pane_titles(), &["nvim", "nv"]);
    }

    #[test]
    fn editor_from_str_works_as_expected() {
        assert2::let_assert!(Ok(Editor::Hx) = Editor::from_str("hx"));
        assert2::let_assert!(Ok(Editor::Nvim) = Editor::from_str("nvim"));
        assert2::let_assert!(Ok(Editor::Nvim) = Editor::from_str("nv"));
        assert2::let_assert!(Err(error) = Editor::from_str("unknown"));
        assert!(error.to_string().contains("unknown editor"));
    }

    #[test]
    fn file_to_open_from_str_works_as_expected() {
        let root_dir = std::env::current_dir().unwrap();
        // We should always have a Cargo.toml...
        let dummy_path = root_dir.join("Cargo.toml").to_string_lossy().to_string();

        assert2::let_assert!(Ok(f0) = FileToOpen::from_str(&dummy_path));
        let expected = FileToOpen {
            path: dummy_path.clone(),
            line_nbr: 0,
            column: 0,
        };
        assert_eq!(f0, expected);

        assert2::let_assert!(Ok(f1) = FileToOpen::from_str(&format!("{dummy_path}:3")));
        let expected = FileToOpen {
            path: dummy_path.clone(),
            line_nbr: 3,
            column: 0,
        };
        assert_eq!(f1, expected);

        assert2::let_assert!(Ok(f2) = FileToOpen::from_str(&format!("{dummy_path}:3:7")));
        let expected = FileToOpen {
            path: dummy_path,
            line_nbr: 3,
            column: 7,
        };
        assert_eq!(f2, expected);
    }

    #[test]
    fn try_from_errors_when_pane_is_missing() {
        let panes: Vec<WeztermPane> = vec![];
        assert2::let_assert!(Err(error) = FileToOpen::try_from(("README.md", 999, panes.as_slice())));
        assert!(error.to_string().contains("missing pane"));
    }

    #[test]
    fn try_from_errors_when_relative_file_is_missing() {
        let dir = std::env::current_dir().unwrap();
        let panes = vec![pane_with(1, 1, &dir)];
        assert2::let_assert!(
            Err(error) = FileToOpen::try_from(("definitely_missing_12345__file.rs", 1, panes.as_slice()))
        );
        assert!(error.to_string().contains("file missing"));
    }

    #[test]
    fn try_from_resolves_relative_existing_file() {
        let dir = std::env::current_dir().unwrap();
        let panes = vec![pane_with(7, 1, &dir)];
        assert2::let_assert!(Ok(file) = FileToOpen::try_from(("Cargo.toml", 7, panes.as_slice())));
        let expected = FileToOpen {
            path: dir.join("Cargo.toml").to_string_lossy().to_string(),
            line_nbr: 0,
            column: 0,
        };
        assert_eq!(file, expected);
    }

    fn pane_with(pane_id: i64, tab_id: i64, cwd_fs: &std::path::Path) -> WeztermPane {
        WeztermPane {
            cursor_shape: "Block".into(),
            cursor_visibility: "Visible".into(),
            cursor_x: 0,
            cursor_y: 0,
            // Use double-slash host form so absolute_cwd drops the first two components and yields the real fs path.
            cwd: PathBuf::from(format!("file://host{}", cwd_fs.display())),
            is_active: true,
            is_zoomed: false,
            left_col: 0,
            pane_id,
            size: WeztermPaneSize {
                cols: 80,
                dpi: 96,
                pixel_height: 800,
                pixel_width: 600,
                rows: 24,
            },
            tab_id,
            tab_title: "tab".into(),
            title: "hx".into(),
            top_row: 0,
            tty_name: "tty".into(),
            window_id: 1,
            window_title: "win".into(),
            workspace: "default".into(),
        }
    }
}
