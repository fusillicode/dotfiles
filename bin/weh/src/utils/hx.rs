use std::io::BufRead;
use std::io::Lines;
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

const ANSI_ESCAPE_SELECTION_BG_COLOR: &str = "[48:2::45:54:64m";

fn get_line_number_from_ansi_escaped_line(ansi_escaped_line: &str) -> anyhow::Result<usize> {
    strip_ansi_escapes::strip_str(ansi_escaped_line)
        .split_whitespace()
        .find_map(|x| x.parse::<usize>().ok())
        .ok_or_else(|| anyhow!("no line number found in line '{ansi_escaped_line}'"))
}

fn get_selection_range<B: BufRead>(
    hx_pane_ansi_escaped_viewport: &[u8],
    actual_selection: Lines<B>,
) -> anyhow::Result<Range<usize>> {
    let hx_pane_ansi_escaped_viewport = String::from_utf8_lossy(hx_pane_ansi_escaped_viewport);

    let hx_cursor = HxCursor::from_str(&strip_ansi_escapes::strip_str(
        hx_pane_ansi_escaped_viewport
            .lines()
            .nth_back(2)
            .ok_or_else(|| {
                anyhow!(
                    "no hx status line in pane hx pane ANSI escaped viewport {hx_pane_ansi_escaped_viewport}",
                )
            })?,
    ))?;

    let mut viewport_lines = hx_pane_ansi_escaped_viewport.lines().enumerate();
    let (viewport_selection_start_line_idx, viewport_selection_start_line) = viewport_lines
        .find_map(|(idx, line)| {
            line.contains(ANSI_ESCAPE_SELECTION_BG_COLOR)
                .then_some((idx, line))
        })
        .unwrap();
    let (viewport_selection_end_line_idx, viewport_selection_end_line) = viewport_lines
        .find_map(|(idx, line)| {
            line.contains(ANSI_ESCAPE_SELECTION_BG_COLOR)
                .then_some((idx, line))
        })
        .unwrap();
    let viewport_selection_size =
        viewport_selection_end_line_idx - viewport_selection_start_line_idx;

    let actual_selection_size = actual_selection.count();

    // If the actual selection is completely in the viewport
    if actual_selection_size == viewport_selection_size {
        let viewport_selection_start_line_number =
            get_line_number_from_ansi_escaped_line(viewport_selection_start_line).unwrap();
        let viewport_selection_end_line_number =
            get_line_number_from_ansi_escaped_line(viewport_selection_end_line).unwrap();

        return Ok(viewport_selection_start_line_number..viewport_selection_end_line_number);
    }

    // If the actual selection expand before the viewport
    let selection = if viewport_selection_start_line_idx == 0 {
        hx_cursor.position.line - actual_selection_size..hx_cursor.position.line
    // If the actual selection expand after the viewport
    } else {
        hx_cursor.position.line..hx_cursor.position.line + actual_selection_size
    };

    // Sanity check if actual selection is smaller than the viewport selection. This should not happen
    if actual_selection_size < viewport_selection_size {
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
    fn test_get_line_number_from_ansi_escaped_line_returns_the_expected_line_number() {
        let result = get_line_number_from_ansi_escaped_line(
            "[38:2::230:180:80m[48:2::15:20:25m●[39m [38:2::102:102:102m42[39m [38:2::170:217:76m▍[38:2::45:54:64m········[38:2::255:143:64mlet[38:2::45:54:64m·(B[0m[38:2::191:189:182m[48:2::15:20:25mselection_start_line(B[0m[38:2::102:102:102m[48:2::15:20:25m:[38:2::45:54:64m [38:2::102:102:102mi32[38:2::45:54:64m·[38:2::255:143:64m=[38:2::45:54:64m·[38:2::210:166:255m190[38:2::191:189:182m;[38:2::45:54:64m [39m                                                                           [48:2::19:23:33m[K",
        );

        assert_eq!(42, result.unwrap());
    }

    #[test]
    fn test_get_line_number_from_ansi_escaped_line_returns_the_expected_error_if_doenst_find_a_line_number(
    ) {
        let result = get_line_number_from_ansi_escaped_line("");

        assert!(result.is_err());
    }

    // #[test]
    // fn test_get_selection_range_returns_the_expected_range_if_the_selection_is_completely_contained_in_the_viewport(
    // ) {
    //     let result = get_selection_range();

    //     assert_eq!((1..2), result.unwrap());
    // }

    // #[test]
    // fn test_get_selection_range_returns_the_expected_range_if_the_selection_expand_before_the_viewport(
    // ) {
    //     let result = get_selection_range();

    //     assert_eq!((1..2), result.unwrap());
    // }

    // #[test]
    // fn test_get_selection_range_returns_the_expected_range_if_the_selection_expand_after_the_viewport(
    // ) {
    //     let result = get_selection_range();

    //     assert_eq!((1..2), result.unwrap());
    // }

    // #[test]
    // fn test_get_selection_range_returns_the_expected_error_if_the_selection_is_completely_contained_in_the_viewport_and_is_smaller_than_the_actual_selection(
    // ) {
    //     let result = get_selection_range();

    //     assert_eq!((1..2), result.unwrap());
    // }
}
