use std::ops::Range;
use std::path::PathBuf;
use std::str::FromStr;
use std::str::SplitWhitespace;

use anyhow::anyhow;
use anyhow::bail;

#[derive(Debug, PartialEq)]
#[cfg_attr(any(test), derive(fake::Dummy))]
pub struct HxStatusLine {
    pub file_path: PathBuf,
    pub position: HxCursorPosition,
}

impl FromStr for HxStatusLine {
    type Err = anyhow::Error;

    fn from_str(hx_status_line: &str) -> Result<Self, Self::Err> {
        let hx_status_line = hx_status_line.trim();

        let elements: Vec<&str> = hx_status_line.split_ascii_whitespace().collect();

        let path_left_separator_idx = elements.iter().position(|x| x == &"`").ok_or_else(|| {
            anyhow!("no left path separator in status line elements {elements:?}")
        })?;
        let path_right_separator_idx =
            elements.iter().rposition(|x| x == &"`").ok_or_else(|| {
                anyhow!("no right path separator in status line elements {elements:?}")
            })?;

        let &["`", path] = &elements[path_left_separator_idx..path_right_separator_idx] else {
            bail!("no path in status line elements {elements:?}");
        };

        Ok(Self {
            file_path: path.into(),
            position: HxCursorPosition::from_str(
                elements.last().ok_or_else(|| {
                    anyhow!("no last element in status line elements {elements:?}")
                })?,
            )?,
        })
    }
}

#[derive(Debug, PartialEq)]
#[cfg_attr(any(test), derive(fake::Dummy))]
pub struct HxCursorPosition {
    pub line: usize,
    pub column: usize,
}

impl FromStr for HxCursorPosition {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (line, column) = s
            .split_once(':')
            .ok_or_else(|| anyhow!("no line column delimiter found in str '{s}'"))?;

        Ok(Self {
            line: line.parse()?,
            column: column.parse()?,
        })
    }
}

const ANSI_ESCAPED_SELECTION_BG_COLOR: &str = "[48:2::45:54:64m";

#[derive(Debug, PartialEq)]
enum SelectionDirection {
    Up,
    Down,
}

impl SelectionDirection {
    fn parse_line_number(stripped_line_parts: &mut SplitWhitespace<'_>) -> anyhow::Result<usize> {
        fn parse_next_part_as_usize(line_parts: &mut SplitWhitespace<'_>) -> Option<usize> {
            line_parts.next().and_then(|x| x.parse::<usize>().ok())
        }

        if let Some(line_number) = parse_next_part_as_usize(stripped_line_parts) {
            return Ok(line_number);
        }
        if let Some(line_number) = parse_next_part_as_usize(stripped_line_parts) {
            return Ok(line_number);
        }

        bail!(
            "missing line number in '{:?}', line number expected in 1st or 2nd position",
            stripped_line_parts.collect::<Vec<&str>>()
        )
    }

    fn is_line_selected(line: &str) -> bool {
        line.contains(ANSI_ESCAPED_SELECTION_BG_COLOR)
    }
}

impl TryFrom<(usize, &str)> for SelectionDirection {
    type Error = anyhow::Error;

    fn try_from(
        (hx_cursor_line_number, hx_pane_ansi_escaped_viewport): (usize, &str),
    ) -> Result<Self, Self::Error> {
        let prev_hx_cursor_line_number = hx_cursor_line_number - 1;
        let next_hx_cursor_line_number = hx_cursor_line_number + 1;

        let mut lines: Vec<_> = hx_pane_ansi_escaped_viewport
            .lines()
            .rev()
            .skip(3)
            .collect();
        lines.reverse();

        for line in lines {
            let stripped_line = strip_ansi_escapes::strip_str(line);
            let mut stripped_line_parts = stripped_line.split_whitespace();
            let line_number = Self::parse_line_number(&mut stripped_line_parts)?;

            if line_number == prev_hx_cursor_line_number && Self::is_line_selected(line) {
                return Ok(SelectionDirection::Up);
            }
            if line_number == next_hx_cursor_line_number && Self::is_line_selected(line) {
                return Ok(SelectionDirection::Down);
            }
        }

        bail!("cannot get selection direction from cursor line number {hx_cursor_line_number} and ansi escaped viewport")
    }
}

fn get_selection_range(
    hx_pane_ansi_escaped_viewport: &str,
    hx_cursor_line_number: usize,
    hx_actual_selection_size: usize,
) -> anyhow::Result<Range<usize>> {
    if hx_actual_selection_size == 1 {
        return Ok(hx_cursor_line_number..hx_cursor_line_number);
    }

    Ok(
        match SelectionDirection::try_from((hx_cursor_line_number, hx_pane_ansi_escaped_viewport))?
        {
            SelectionDirection::Up => {
                hx_cursor_line_number - hx_actual_selection_size..hx_cursor_line_number
            }
            SelectionDirection::Down => {
                hx_cursor_line_number..hx_cursor_line_number + hx_actual_selection_size
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hx_cursor_from_str_works_as_expected_with_a_file_path_pointing_to_an_existent_file_in_normal_mode(
    ) {
        let result = HxStatusLine::from_str("      ● 1 ` src/utils.rs `                                                                  1 sel  1 char  W ● 1  42:33 ");
        let expected = HxStatusLine {
            file_path: "src/utils.rs".into(),
            position: HxCursorPosition {
                line: 42,
                column: 33,
            },
        };

        assert_eq!(expected, result.unwrap());
    }

    #[test]
    fn test_hx_cursor_from_str_works_as_expected_with_a_file_path_pointing_to_an_existent_file_and_a_spinner(
    ) {
        let result = HxStatusLine::from_str("⣷      ` src/utils.rs `                                                                  1 sel  1 char  W ● 1  33:42 ");
        let expected = HxStatusLine {
            file_path: "src/utils.rs".into(),
            position: HxCursorPosition {
                line: 33,
                column: 42,
            },
        };

        assert_eq!(expected, result.unwrap());
    }

    #[test]
    fn test_selection_direction_try_from_returns_up_if_selection_expands_up_to_the_supplied_cursor_line(
    ) {
        let result = SelectionDirection::try_from((
            29,
            std::fs::read_to_string("./fixtures/ansi_escaped_selection_expands_up.txt")
                .unwrap()
                .as_str(),
        ));

        assert_eq!(SelectionDirection::Up, result.unwrap());
    }

    #[test]
    fn test_selection_direction_try_from_returns_down_if_selection_expands_down_to_the_supplied_cursor_line(
    ) {
        let result = SelectionDirection::try_from((
            14,
            std::fs::read_to_string("./fixtures/ansi_escaped_selection_expands_down.txt")
                .unwrap()
                .as_str(),
        ));

        assert_eq!(SelectionDirection::Down, result.unwrap());
    }

    #[test]
    fn test_selection_direction_try_from_returns_an_error_if_a_line_number_is_missing() {
        let result = SelectionDirection::try_from((
            29,
            std::fs::read_to_string("./fixtures/ansi_escaped_selection_missing_line_number.txt")
                .unwrap()
                .as_str(),
        ));

        let error_message = result.unwrap_err().to_string();

        assert!(error_message.contains("missing line number in "));
        assert!(error_message.contains(", line number expected in 1st or 2nd position"));
    }

    #[test]
    fn test_selection_direction_try_from_returns_an_error_if_selection_is_one_single_line() {
        let result = SelectionDirection::try_from((
            14,
            std::fs::read_to_string("./fixtures/ansi_escaped_selection_single_line.txt")
                .unwrap()
                .as_str(),
        ));

        assert_eq!(
            "cannot get selection direction from cursor line number 14 and ansi escaped viewport",
            result.unwrap_err().to_string()
        );
    }

    #[test]
    fn test_selection_direction_try_from_returns_an_error_if_there_is_no_selection() {
        let result = SelectionDirection::try_from((
            65,
            std::fs::read_to_string("./fixtures/ansi_escaped_no_selection.txt")
                .unwrap()
                .as_str(),
        ));

        assert_eq!(
            "cannot get selection direction from cursor line number 65 and ansi escaped viewport",
            result.unwrap_err().to_string()
        );
    }
}
