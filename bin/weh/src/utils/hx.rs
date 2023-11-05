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
enum SelectionAroundHxCursor {
    ExpandsUp,
    ExpandsDown,
}

impl SelectionAroundHxCursor {
    fn find_idx_and_line_matching_line_number(
        hx_pane_ansi_stripped_viewport: &str,
        hx_cursor_line_number: usize,
    ) -> Option<(usize, &str)> {
        fn parse_next_as_usize(line_parts: &mut SplitWhitespace<'_>) -> Option<usize> {
            line_parts.next().and_then(|x| x.parse::<usize>().ok())
        }

        hx_pane_ansi_stripped_viewport
            .lines()
            .enumerate()
            .find(|(_, line)| {
                let mut line_parts = line.split_whitespace();

                parse_next_as_usize(&mut line_parts)
                    .is_some_and(|line_number| line_number == hx_cursor_line_number)
                    || parse_next_as_usize(&mut line_parts)
                        .is_some_and(|line_number| line_number == hx_cursor_line_number)
            })
    }

    fn is_line_selected(line: &str) -> bool {
        line.contains(ANSI_ESCAPED_SELECTION_BG_COLOR)
    }
}

impl TryFrom<(usize, &str)> for SelectionAroundHxCursor {
    type Error = anyhow::Error;

    fn try_from(
        (hx_cursor_line_number, hx_pane_ansi_escaped_viewport): (usize, &str),
    ) -> Result<Self, Self::Error> {
        let hx_pane_ansi_stripped_viewport =
            strip_ansi_escapes::strip_str(hx_pane_ansi_escaped_viewport);

        // ANSI escaped viewport and stripped one should have the same length
        let ansi_escaped_lines_count = hx_pane_ansi_escaped_viewport.lines().count();
        let ansi_stripped_lines_count = hx_pane_ansi_stripped_viewport.lines().count();
        if ansi_stripped_lines_count != ansi_escaped_lines_count {
            bail!("lines count of ANSI stripped '{ansi_stripped_lines_count}' doesn't match ANSI escaped viewport one '{ansi_escaped_lines_count}'")
        }

        let (hx_cursor_line_idx, _) = Self::find_idx_and_line_matching_line_number(
            &hx_pane_ansi_stripped_viewport,
            hx_cursor_line_number,
        )
        .ok_or_else(|| {
            anyhow!("cannot find line number '{hx_cursor_line_number}' in ANSI stripped viewport")
        })?;

        let mut lines = hx_pane_ansi_escaped_viewport.lines();
        let prev_line_idx = hx_cursor_line_idx - 1;
        let prev_line = lines
            .nth(prev_line_idx)
            .ok_or_else(|| anyhow!("cannot find prev line of hx cursor, idx '{prev_line_idx}'"))?;
        if Self::is_line_selected(prev_line) {
            return Ok(SelectionAroundHxCursor::ExpandsUp);
        }

        let next_line_idx = hx_cursor_line_idx - 1;
        let next_line = lines
            .nth(next_line_idx)
            .ok_or_else(|| anyhow!("cannot find next of hx cursor, idx '{next_line_idx}'"))?;
        if Self::is_line_selected(next_line) {
            return Ok(SelectionAroundHxCursor::ExpandsDown);
        }

        bail!("cannot get selection direction from cursor line number {hx_cursor_line_number} in ANSI escaped viewport")
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
        match SelectionAroundHxCursor::try_from((
            hx_cursor_line_number,
            hx_pane_ansi_escaped_viewport,
        ))? {
            SelectionAroundHxCursor::ExpandsUp => {
                hx_cursor_line_number - hx_actual_selection_size..hx_cursor_line_number
            }
            SelectionAroundHxCursor::ExpandsDown => {
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
        let result = SelectionAroundHxCursor::try_from((
            29,
            std::fs::read_to_string("./fixtures/ansi_escaped_selection_expands_up.txt")
                .unwrap()
                .as_str(),
        ));

        assert_eq!(SelectionAroundHxCursor::ExpandsUp, result.unwrap());
    }

    #[test]
    fn test_selection_direction_try_from_returns_down_if_selection_expands_down_to_the_supplied_cursor_line(
    ) {
        let result = SelectionAroundHxCursor::try_from((
            14,
            std::fs::read_to_string("./fixtures/ansi_escaped_selection_expands_down.txt")
                .unwrap()
                .as_str(),
        ));

        assert_eq!(SelectionAroundHxCursor::ExpandsDown, result.unwrap());
    }

    #[test]
    fn test_selection_direction_try_from_returns_an_error_if_hx_cursor_line_number_is_missing() {
        let result = SelectionAroundHxCursor::try_from((
            29,
            std::fs::read_to_string("./fixtures/ansi_escaped_selection_missing_line_number.txt")
                .unwrap()
                .as_str(),
        ));

        assert_eq!(
            "cannot find line number '29' in ANSI stripped viewport",
            result.unwrap_err().to_string()
        );
    }

    #[test]
    fn test_selection_direction_try_from_returns_an_error_if_selection_is_one_single_line() {
        let result = SelectionAroundHxCursor::try_from((
            14,
            std::fs::read_to_string("./fixtures/ansi_escaped_single_line_selection.txt")
                .unwrap()
                .as_str(),
        ));

        assert_eq!(
            "cannot get selection direction from cursor line number 14 in ANSI escaped viewport",
            result.unwrap_err().to_string()
        );
    }

    #[test]
    fn test_selection_direction_try_from_returns_an_error_if_there_is_no_selection() {
        let result = SelectionAroundHxCursor::try_from((
            14,
            std::fs::read_to_string("./fixtures/ansi_escaped_no_selection.txt")
                .unwrap()
                .as_str(),
        ));

        assert_eq!(
            "cannot get selection direction from cursor line number 14 in ANSI escaped viewport",
            result.unwrap_err().to_string()
        );
    }
}
