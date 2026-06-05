use std::num::NonZeroU16;

use rootcause::report;
use serde::Deserialize;
use serde::Serialize;

#[derive(rkyv::Archive, Clone, Debug, Deserialize, rkyv::Deserialize, Eq, PartialEq, Serialize, rkyv::Serialize)]
pub struct TerminalSize {
    cols: NonZeroU16,
    rows: NonZeroU16,
}

impl TerminalSize {
    /// Build terminal dimensions, rejecting zero values before they reach the PTY layer.
    ///
    /// # Errors
    /// - Columns or rows are zero.
    pub fn new(cols: u16, rows: u16) -> rootcause::Result<Self> {
        let Some(cols) = NonZeroU16::new(cols) else {
            return Err(report!("invalid muxr terminal size").attach("cols=0"));
        };
        let Some(rows) = NonZeroU16::new(rows) else {
            return Err(report!("invalid muxr terminal size").attach("rows=0"));
        };

        Ok(Self { cols, rows })
    }

    /// Return terminal columns.
    #[must_use]
    pub const fn cols(&self) -> u16 {
        self.cols.get()
    }

    /// Return terminal rows.
    #[must_use]
    pub const fn rows(&self) -> u16 {
        self.rows.get()
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::zero_cols(r#"{"cols":0,"rows":24}"#)]
    #[case::zero_rows(r#"{"cols":80,"rows":0}"#)]
    fn test_terminal_size_deserialize_when_dimension_is_zero_returns_error(#[case] raw: &str) {
        assert2::assert!(serde_json::from_str::<TerminalSize>(raw).is_err());
    }

    #[rstest]
    #[case::zero_cols(0, 24)]
    #[case::zero_rows(80, 0)]
    fn test_terminal_size_new_when_dimension_is_zero_returns_error(#[case] cols: u16, #[case] rows: u16) {
        assert2::assert!(TerminalSize::new(cols, rows).is_err());
    }

    #[test]
    fn test_terminal_size_new_when_dimensions_are_nonzero_returns_size() -> rootcause::Result<()> {
        let size = TerminalSize::new(120, 40)?;

        pretty_assertions::assert_eq!(size.cols(), 120);
        pretty_assertions::assert_eq!(size.rows(), 40);
        Ok(())
    }
}
