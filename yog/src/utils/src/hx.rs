use std::path::PathBuf;
use core::str::FromStr;

use color_eyre::eyre;
use color_eyre::eyre::bail;
use color_eyre::eyre::eyre;

/// Represents the parsed status line from Helix editor, containing file path and cursor position.
#[derive(Debug, PartialEq)]
#[cfg_attr(any(test, feature = "fake"), derive(fake::Dummy))]
pub struct HxStatusLine {
    /// The file path currently open in the editor.
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
            .ok_or_else(|| eyre!("missing left path separator in status line elements {elements:#?}"))?;
        let path_right_separator_idx = elements
            .iter()
            .rposition(|x| x == &"`")
            .ok_or_else(|| eyre!("missing right path separator in status line elements {elements:#?}"))?;

        let &["`", path] = &elements[path_left_separator_idx..path_right_separator_idx] else {
            bail!("missing path in status line elements {elements:#?}");
        };

        Ok(Self {
            file_path: path.into(),
            position: HxCursorPosition::from_str(
                elements
                    .last()
                    .ok_or_else(|| eyre!("missing last element in status line elements {elements:#?}"))?,
            )?,
        })
    }
}

/// Represents a cursor position in a text file with line and column coordinates.
#[derive(Debug, PartialEq)]
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
            .ok_or_else(|| eyre!("missing line column delimiter found in str '{s}'"))?;

        Ok(Self {
            line: line.parse()?,
            column: column.parse()?,
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

        assert_eq!(expected, result.unwrap());
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

        assert_eq!(expected, result.unwrap());
    }
}
