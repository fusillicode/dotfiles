use std::num::NonZeroU16;

use nutype::nutype;
use rootcause::report;
use serde::Deserialize;
use serde::Serialize;

#[derive(rkyv::Archive, Clone, Debug, Deserialize, rkyv::Deserialize, Eq, PartialEq, Serialize, rkyv::Serialize)]
pub struct TerminalSize {
    cols: TerminalDimension,
    rows: TerminalDimension,
}

impl TerminalSize {
    /// Build terminal dimensions, rejecting zero values before they reach the PTY layer.
    ///
    /// # Errors
    /// - Columns or rows are zero.
    pub fn new(cols: u16, rows: u16) -> rootcause::Result<Self> {
        let cols =
            TerminalDimension::try_new(cols).map_err(|_| report!("invalid muxr terminal size").attach("cols=0"))?;
        let rows =
            TerminalDimension::try_new(rows).map_err(|_| report!("invalid muxr terminal size").attach("rows=0"))?;

        Ok(Self { cols, rows })
    }

    /// Return terminal columns.
    #[must_use]
    pub const fn cols(&self) -> u16 {
        self.cols.into_inner()
    }

    /// Return terminal rows.
    #[must_use]
    pub const fn rows(&self) -> u16 {
        self.rows.into_inner()
    }
}

#[nutype(
    const_fn,
    validate(greater = 0),
    derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)
)]
pub(super) struct TerminalDimension(u16);

impl From<TerminalDimension> for NonZeroU16 {
    fn from(value: TerminalDimension) -> Self {
        // Nutype validates every construction path; zero cannot reach this infallible conversion.
        Self::new(value.into_inner()).unwrap_or(Self::MIN)
    }
}

impl rkyv::Archive for TerminalDimension {
    type Archived = rkyv::Archived<NonZeroU16>;
    type Resolver = rkyv::Resolver<NonZeroU16>;

    fn resolve(&self, resolver: Self::Resolver, out: rkyv::Place<Self::Archived>) {
        rkyv::Archive::resolve(&NonZeroU16::from(*self), resolver, out);
    }
}

impl<S> rkyv::Serialize<S> for TerminalDimension
where
    S: rkyv::rancor::Fallible + ?Sized,
    NonZeroU16: rkyv::Serialize<S>,
{
    fn serialize(&self, serializer: &mut S) -> Result<Self::Resolver, S::Error> {
        rkyv::Serialize::serialize(&NonZeroU16::from(*self), serializer)
    }
}

impl<D> rkyv::Deserialize<TerminalDimension, D> for rkyv::primitive::ArchivedNonZeroU16
where
    D: rkyv::rancor::Fallible + ?Sized,
    D::Error: rkyv::rancor::Source,
{
    fn deserialize(&self, _deserializer: &mut D) -> Result<TerminalDimension, D::Error> {
        TerminalDimension::try_new(self.to_native().get()).map_err(super::rkyv_deserialize_error::<D::Error>)
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use test_that::prelude::*;

    use super::*;

    #[derive(rkyv::Archive, rkyv::Serialize)]
    struct RawTerminalSize {
        cols: u16,
        rows: u16,
    }

    #[rstest::rstest]
    #[case::zero_cols(0, 24)]
    #[case::zero_rows(80, 0)]
    fn test_terminal_size_archive_when_zero_returns_error(
        #[case] cols: u16,
        #[case] rows: u16,
    ) -> rootcause::Result<()> {
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&RawTerminalSize { cols, rows })?;
        assert_that!(
            rkyv::access::<rkyv::Archived<TerminalSize>, rkyv::rancor::Error>(&bytes).map(|_| ()),
            err(anything())
        );
        assert_that!(
            rkyv::from_bytes::<TerminalSize, rkyv::rancor::Error>(&bytes),
            err(anything())
        );
        Ok(())
    }

    #[rstest]
    #[case::zero_cols(r#"{"cols":0,"rows":24}"#)]
    #[case::zero_rows(r#"{"cols":80,"rows":0}"#)]
    fn test_terminal_size_deserialize_when_dimension_is_zero_returns_error(#[case] raw: &str) {
        assert_that!(serde_json::from_str::<TerminalSize>(raw), err(anything()));
    }

    #[rstest]
    #[case::zero_cols(0, 24)]
    #[case::zero_rows(80, 0)]
    fn test_terminal_size_new_when_dimension_is_zero_returns_error(#[case] cols: u16, #[case] rows: u16) {
        assert_that!(TerminalSize::new(cols, rows), err(anything()));
    }

    #[test]
    fn test_terminal_size_new_when_dimensions_are_nonzero_returns_size() -> rootcause::Result<()> {
        let size = TerminalSize::new(120, 40)?;

        assert_that!(size.cols(), eq(120));
        assert_that!(size.rows(), eq(40));
        Ok(())
    }
}
