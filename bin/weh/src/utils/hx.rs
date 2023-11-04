use std::ops::Range;
use std::path::PathBuf;
use std::str::FromStr;

use anyhow::anyhow;
use anyhow::bail;

#[derive(Debug, PartialEq)]
#[cfg_attr(any(test), derive(fake::Dummy))]
pub struct HxCursor {
    pub file_path: PathBuf,
    pub position: HxCursorPosition,
}

impl FromStr for HxCursor {
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
const ANSI_ESCAPED_BG_COLOR: &str = "[48:2::45:54:64m";

#[derive(Debug)]
struct HxViewportSelectionLine<'a> {
    idx: usize,
    content: &'a str,
}

impl<'a> HxViewportSelectionLine<'a> {
    pub fn next_matching_background_in(
        mut viewport_lines: std::iter::Enumerate<std::str::Lines<'a>>,
        ansi_escaped_background_color: &str,
    ) -> Option<(std::iter::Enumerate<std::str::Lines<'a>>, Self)> {
        if let Some(viewport_selection_line) = viewport_lines
            .find_map(|(idx, line)| {
                line.contains(ansi_escaped_background_color)
                    .then_some((idx, line))
            })
            .map(|(idx, content)| Self { idx, content })
        {
            return Some((viewport_lines, viewport_selection_line));
        }
        None
    }

    pub fn nbr_of_lines_from(&self, other: &Self) -> usize {
        self.idx.abs_diff(other.idx)
    }

    pub fn get_line_number(&self) -> anyhow::Result<usize> {
        strip_ansi_escapes::strip_str(self.content)
            .split_whitespace()
            .find_map(|x| x.parse::<usize>().ok())
            .ok_or_else(|| anyhow!("no line number found in line '{}'", self.content))
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

    let viewport_lines = hx_pane_ansi_escaped_viewport.lines().enumerate();
    let (viewport_lines, viewport_selection_line_start) =
        HxViewportSelectionLine::next_matching_background_in(viewport_lines).unwrap();
    let (_, viewport_selection_line_end) =
        HxViewportSelectionLine::next_matching_background_in(viewport_lines).unwrap();
    let viewport_selection_size =
        viewport_selection_line_start.nbr_of_lines_from(&viewport_selection_line_end);

    dbg!(&viewport_selection_line_start);
    dbg!(&viewport_selection_line_end);

    // If the actual selection is fully contained in the viewport
    if hx_actual_selection_size == viewport_selection_size {
        return Ok(viewport_selection_line_start.get_line_number().unwrap()
            ..viewport_selection_line_end.get_line_number().unwrap());
    }

    // If the actual selection expand before the viewport
    let selection = if viewport_selection_line_start.idx == 0 {
        hx_cursor_line_number - hx_actual_selection_size..hx_cursor_line_number
    // If the actual selection expand after the viewport
    } else {
        hx_cursor_line_number..hx_cursor_line_number + hx_actual_selection_size
    };

    // Sanity check if actual selection is smaller than the viewport selection. This should not happen.
    if hx_actual_selection_size < viewport_selection_size {
        bail!("foo")
    }

    Ok(selection)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hx_cursor_from_str_works_as_expected_with_a_file_path_pointing_to_an_existent_file_in_normal_mode(
    ) {
        let result = HxCursor::from_str("      ● 1 ` src/utils.rs `                                                                  1 sel  1 char  W ● 1  42:33 ");
        let expected = HxCursor {
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
        let result = HxCursor::from_str("⣷      ` src/utils.rs `                                                                  1 sel  1 char  W ● 1  33:42 ");
        let expected = HxCursor {
            file_path: "src/utils.rs".into(),
            position: HxCursorPosition {
                line: 33,
                column: 42,
            },
        };

        assert_eq!(expected, result.unwrap());
    }

    #[test]
    fn test_get_line_number_returns_the_expected_line_number() {
        let result = HxViewportSelectionLine {
            idx: 0,
            content: "[38:2::230:180:80m[48:2::15:20:25m●[39m [38:2::102:102:102m42[39m [38:2::170:217:76m▍[38:2::45:54:64m········[38:2::255:143:64mlet[38:2::45:54:64m·(B[0m[38:2::191:189:182m[48:2::15:20:25mselection_start_line(B[0m[38:2::102:102:102m[48:2::15:20:25m:[38:2::45:54:64m [38:2::102:102:102mi32[38:2::45:54:64m·[38:2::255:143:64m=[38:2::45:54:64m·[38:2::210:166:255m190[38:2::191:189:182m;[38:2::45:54:64m [39m                                                                           [48:2::19:23:33m[K"
        }.get_line_number();

        assert_eq!(42, result.unwrap());
    }

    #[test]
    fn test_get_line_number_returns_the_expected_error_if_doenst_find_a_line_number() {
        let result = HxViewportSelectionLine {
            idx: 0,
            content: "",
        }
        .get_line_number();

        assert!(result.is_err());
    }

    #[test]
    fn test_get_selection_range_returns_the_expected_range_if_the_actual_selection_is_fully_contained_in_the_viewport(
    ) {
        let result = get_selection_range(
            &std::fs::read_to_string("./fixtures/actual_selection_fully_contained_in_viewport.txt")
                .unwrap(),
            113,
            7,
        );

        assert_eq!((108..113), result.unwrap());
    }

    #[test]
    fn test_get_selection_range_returns_the_expected_range_if_the_actual_selection_expands_before_the_viewport(
    ) {
        let result = get_selection_range(
            &std::fs::read_to_string("./fixtures/actual_selection_expands_before_the_viewport.txt")
                .unwrap(),
            3,
            3,
        );

        assert_eq!((1..2), result.unwrap());
    }

    #[test]
    fn test_get_selection_range_returns_the_expected_range_if_the_actual_selection_expands_after_the_viewport(
    ) {
        let result = get_selection_range(
            &std::fs::read_to_string("./fixtures/actual_selection_expands_after_the_viewport.txt")
                .unwrap(),
            3,
            3,
        );

        assert_eq!((1..2), result.unwrap());
    }

    #[test]
    fn test_get_selection_range_returns_the_expected_error_if_the_actual_selection_is_fully_contained_in_the_viewport_but_the_viewport_is_smaller_than_the_actual_selection(
    ) {
        let result = get_selection_range(
            &std::fs::read_to_string("./fixtures/actual_selection_fully_contained_in_viewport.txt")
                .unwrap(),
            3,
            3,
        );

        assert_eq!((1..2), result.unwrap());
    }
}
