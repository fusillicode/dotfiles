//! Parse Helix (hx) status line output into structured types: [`HxStatusLine`] and [`HxCursorPosition`].

use core::str::FromStr;
use std::path::PathBuf;

use color_eyre::eyre;
use color_eyre::eyre::WrapErr;
use color_eyre::eyre::bail;
use color_eyre::eyre::eyre;

/// Represents the parsed status line from Helix editor, containing filepath and cursor position.
#[derive(Debug, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "fake"), derive(fake::Dummy))]
pub struct HxStatusLine {
    /// The filepath currently open in the editor.
    pub file_path: PathBuf,
    /// The current cursor position in the file.
    pub position: HxCursorPosition,
}

/// Parses a [`HxStatusLine`] from a Helix editor status line string.
impl FromStr for HxStatusLine {
    type Err = eyre::Error;

    fn from_str(hx_status_line: &str) -> Result<Self, Self::Err> {
        let hx_status_line = hx_status_line.trim();

        let elements: Vec<&str> = hx_status_line.split_ascii_whitespace().collect();

        let path_left_separator_idx = elements
            .iter()
            .position(|x| x == &"`")
            .ok_or_else(|| eyre!("error missing left path separator | elements={elements:#?}"))?;
        let path_right_separator_idx = elements
            .iter()
            .rposition(|x| x == &"`")
            .ok_or_else(|| eyre!("error missing right path separator | elements={elements:#?}"))?;

        let path_slice_range = path_left_separator_idx..path_right_separator_idx;
        let path_slice = elements
            .get(path_slice_range.clone())
            .ok_or_else(|| eyre!("error invalid path slice indices | range={path_slice_range:#?}"))?;
        let ["`", path] = path_slice else {
            bail!("missing path | elements={elements:#?}");
        };

        Ok(Self {
            file_path: path.into(),
            position: HxCursorPosition::from_str(
                elements
                    .last()
                    .ok_or_else(|| eyre!("error missing last element | elements={elements:#?}"))?,
            )?,
        })
    }
}

/// Represents a cursor position in a text file with line and column coordinates.
#[derive(Debug, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "fake"), derive(fake::Dummy))]
pub struct HxCursorPosition {
    /// The column number (1-based).
    pub column: usize,
    /// The line number (1-based).
    pub line: usize,
}

/// Parses a [`HxCursorPosition`] from a string in the format "line:column".
impl FromStr for HxCursorPosition {
    type Err = eyre::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (line, column) = s
            .split_once(':')
            .ok_or_else(|| eyre!("error missing line column delimiter | input={s}"))?;

        Ok(Self {
            line: line
                .parse()
                .wrap_err_with(|| eyre!("invalid line number | input={s:?}"))?,
            column: column
                .parse()
                .wrap_err_with(|| eyre!("invalid column number | input={s:?}"))?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hx_cursor_from_str_works_as_expected_with_a_file_path_pointing_to_an_existent_file_in_normal_mode() {
        let result = HxStatusLine::from_str(
            "      ● 1 ` src/utils.rs `                                                                  1 sel  1 char  W ● 1  42:33 ",
        );
        let expected = HxStatusLine {
            file_path: "src/utils.rs".into(),
            position: HxCursorPosition { line: 42, column: 33 },
        };

        assert_eq!(result.unwrap(), expected);
    }

    #[test]
    fn hx_cursor_from_str_works_as_expected_with_a_file_path_pointing_to_an_existent_file_and_a_spinner() {
        let result = HxStatusLine::from_str(
            "⣷      ` src/utils.rs `                                                                  1 sel  1 char  W ● 1  33:42 ",
        );
        let expected = HxStatusLine {
            file_path: "src/utils.rs".into(),
            position: HxCursorPosition { line: 33, column: 42 },
        };

        assert_eq!(result.unwrap(), expected);
    }
}
