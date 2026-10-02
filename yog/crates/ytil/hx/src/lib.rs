//! Parse Helix (hx) status line output into structured types: [`HxStatusLine`] and [`HxCursorPosition`].

use core::str::FromStr;
use std::path::PathBuf;

#[cfg(any(test, feature = "fake"))]
use fake::Dummy;
#[cfg(any(test, feature = "fake"))]
use fake::Fake;
#[cfg(any(test, feature = "fake"))]
use fake::Faker;
#[cfg(any(test, feature = "fake"))]
use fake::RngExt;
use nutype::nutype;
use rootcause::prelude::ResultExt;
use rootcause::report;

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
    type Err = rootcause::Report;

    fn from_str(hx_status_line: &str) -> Result<Self, Self::Err> {
        let hx_status_line = hx_status_line.trim();

        let elements: Vec<&str> = hx_status_line.split_ascii_whitespace().collect();

        let path_left_separator_idx = elements
            .iter()
            .position(|x| x == &"`")
            .ok_or_else(|| report!("error missing left path separator"))
            .attach_with(|| format!("elements={elements:#?}"))?;
        let path_right_separator_idx = elements
            .iter()
            .rposition(|x| x == &"`")
            .ok_or_else(|| report!("error missing right path separator"))
            .attach_with(|| format!("elements={elements:#?}"))?;

        let path_slice_range = path_left_separator_idx..path_right_separator_idx;
        let path_slice = elements
            .get(path_slice_range.clone())
            .ok_or_else(|| report!("error invalid path slice indices"))
            .attach_with(|| format!("range={path_slice_range:#?}"))?;
        let ["`", path] = path_slice else {
            return Err(report!("missing path").attach(format!("elements={elements:#?}")));
        };

        Ok(Self {
            file_path: path.into(),
            position: HxCursorPosition::from_str(
                elements
                    .last()
                    .ok_or_else(|| report!("error missing last element"))
                    .attach_with(|| format!("elements={elements:#?}"))?,
            )?,
        })
    }
}

/// A 1-based Helix line or column coordinate.
#[nutype(validate(greater = 0), derive(Clone, Copy, Debug, Eq, PartialEq, Display, FromStr))]
pub struct HxCoordinate(usize);

#[cfg(any(test, feature = "fake"))]
impl Dummy<Faker> for HxCoordinate {
    fn dummy_with_rng<R: RngExt + ?Sized>(config: &Faker, rng: &mut R) -> Self {
        loop {
            if let Ok(coordinate) = Self::try_new(config.fake_with_rng::<usize, _>(rng)) {
                return coordinate;
            }
        }
    }
}

/// Represents a cursor position in a text file with line and column coordinates.
#[derive(Debug, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "fake"), derive(fake::Dummy))]
pub struct HxCursorPosition {
    /// The column number (1-based).
    pub column: HxCoordinate,
    /// The line number (1-based).
    pub line: HxCoordinate,
}

/// Parses a [`HxCursorPosition`] from a string in the format "line:column".
impl FromStr for HxCursorPosition {
    type Err = rootcause::Report;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (line, column) = s
            .split_once(':')
            .ok_or_else(|| report!("error missing line column delimiter"))
            .attach_with(|| format!("input={s}"))?;

        Ok(Self {
            line: line
                .parse()
                .context("invalid line number")
                .attach_with(|| format!("input={s:?}"))?,
            column: column
                .parse()
                .context("invalid column number")
                .attach_with(|| format!("input={s:?}"))?,
        })
    }
}

#[cfg(test)]
mod tests {
    use fake::rand::SeedableRng;
    use fake::rand::rngs::StdRng;

    use super::*;

    #[rstest::rstest]
    #[case::zero_line("0:1", "invalid line number")]
    #[case::zero_column("1:0", "invalid column number")]
    #[case::both_zero("0:0", "invalid line number")]
    fn test_hx_cursor_position_when_coordinate_is_zero_returns_error(#[case] input: &str, #[case] expected: &str) {
        let error = input
            .parse::<HxCursorPosition>()
            .expect_err("zero coordinates must be rejected");

        assert_eq!(error.format_current_context().to_string(), expected);
    }

    #[test]
    fn test_hx_coordinate_when_value_is_zero_returns_error() {
        let _error = HxCoordinate::try_new(0).expect_err("zero coordinates must be rejected");
    }

    #[rstest::rstest]
    #[case::minimum(1, 1)]
    #[case::maximum_column(1, usize::MAX)]
    #[case::maximum_line(usize::MAX, 1)]
    #[case::maximum_both(usize::MAX, usize::MAX)]
    fn test_hx_cursor_position_when_coordinates_are_at_bounds_returns_position(
        #[case] line: usize,
        #[case] column: usize,
    ) {
        let expected = HxCursorPosition {
            line: HxCoordinate::try_new(line).expect("positive line must be accepted"),
            column: HxCoordinate::try_new(column).expect("positive column must be accepted"),
        };
        let actual = format!("{line}:{column}")
            .parse::<HxCursorPosition>()
            .expect("positive coordinates must parse");

        assert_eq!(actual, expected);
    }

    #[rstest::rstest]
    #[case::zero_seed(0)]
    #[case::one_seed(1)]
    #[case::ordinary_seed(42)]
    #[case::maximum_seed(u64::MAX)]
    fn test_hx_cursor_position_when_generated_by_faker_returns_positive_coordinates(#[case] seed: u64) {
        let mut rng = StdRng::seed_from_u64(seed);
        let position = Faker.fake_with_rng::<HxCursorPosition, _>(&mut rng);

        assert!(position.line.into_inner() > 0);
        assert!(position.column.into_inner() > 0);
    }

    #[test]
    fn test_hx_cursor_from_str_when_file_exists_in_normal_mode_returns_cursor() {
        let result = HxStatusLine::from_str(
            "      ● 1 ` src/utils.rs `                                                                  1 sel  1 char  W ● 1  42:33 ",
        );
        let expected = HxStatusLine {
            file_path: "src/utils.rs".into(),
            position: HxCursorPosition {
                line: HxCoordinate::try_new(42).expect("test coordinate should be valid"),
                column: HxCoordinate::try_new(33).expect("test coordinate should be valid"),
            },
        };

        assert_eq!(result.unwrap(), expected);
    }

    #[test]
    fn test_hx_cursor_from_str_when_file_exists_with_spinner_returns_cursor() {
        let result = HxStatusLine::from_str(
            "⣷      ` src/utils.rs `                                                                  1 sel  1 char  W ● 1  33:42 ",
        );
        let expected = HxStatusLine {
            file_path: "src/utils.rs".into(),
            position: HxCursorPosition {
                line: HxCoordinate::try_new(33).expect("test coordinate should be valid"),
                column: HxCoordinate::try_new(42).expect("test coordinate should be valid"),
            },
        };

        assert_eq!(result.unwrap(), expected);
    }
}
