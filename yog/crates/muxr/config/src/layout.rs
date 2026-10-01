//! Initial pane split proportions and resize increments.

use nutype::nutype;

pub const SPLIT_RATIO_MIN_PER_MILLE: u16 = 50;
pub const SPLIT_RATIO_MAX_PER_MILLE: u16 = 950;
const SPLIT_RESIZE_STEP_MIN: u16 = 1;
const SPLIT_RESIZE_STEP_MAX: u16 = 950;

/// Pane layout tuning.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LayoutConfig {
    pub horizontal_split_ratio: SplitRatio,
    pub resize_step: SplitResizeStep,
    pub vertical_split_ratio: SplitRatio,
}

impl LayoutConfig {
    /// Build the static pane layout configuration.
    ///
    /// # Errors
    /// Returns an error if a configured split ratio or resize step is outside its supported bounds.
    pub fn new() -> rootcause::Result<Self> {
        Ok(Self {
            horizontal_split_ratio: SplitRatio::new(500)?,
            resize_step: SplitResizeStep::new(50)?,
            vertical_split_ratio: SplitRatio::new(400)?,
        })
    }
}

/// A pane split ratio expressed in parts per thousand.
#[nutype(
    const_fn,
    validate(greater_or_equal = SPLIT_RATIO_MIN_PER_MILLE, less_or_equal = SPLIT_RATIO_MAX_PER_MILLE),
    derive(Clone, Copy, Debug, Eq, PartialEq, TryFrom),
)]
pub struct SplitRatio(u16);

impl SplitRatio {
    /// Build a split ratio in parts per thousand.
    ///
    /// # Errors
    /// Returns an error when `value` is outside the range supported by muxr pane layout.
    pub fn new(value: u16) -> rootcause::Result<Self> {
        Self::try_new(value).map_err(|_| {
            rootcause::report!("muxr split ratio is outside supported bounds")
                .attach(format!("min={SPLIT_RATIO_MIN_PER_MILLE}"))
                .attach(format!("max={SPLIT_RATIO_MAX_PER_MILLE}"))
                .attach(format!("actual={value}"))
        })
    }

    /// Return the split ratio in parts per thousand.
    pub const fn per_mille(self) -> u16 {
        self.into_inner()
    }
}

/// A pane split resize delta expressed in parts per thousand.
#[nutype(
    const_fn,
    validate(greater_or_equal = SPLIT_RESIZE_STEP_MIN, less_or_equal = SPLIT_RESIZE_STEP_MAX),
    derive(Clone, Copy, Debug, Eq, PartialEq, TryFrom),
)]
pub struct SplitResizeStep(u16);

impl SplitResizeStep {
    /// Build a split resize delta in parts per thousand.
    ///
    /// # Errors
    /// Returns an error when `value` is zero or larger than the supported split-ratio range.
    pub fn new(value: u16) -> rootcause::Result<Self> {
        Self::try_new(value).map_err(|_| {
            rootcause::report!("muxr split resize step is outside supported bounds")
                .attach(format!("min={SPLIT_RESIZE_STEP_MIN}"))
                .attach(format!("max={SPLIT_RESIZE_STEP_MAX}"))
                .attach(format!("actual={value}"))
        })
    }

    /// Return the resize step in parts per thousand.
    pub const fn per_mille(self) -> u16 {
        self.into_inner()
    }
}

#[cfg(test)]
mod tests {
    use test_that::prelude::*;

    use super::*;

    #[rstest::rstest]
    #[case::below_min(49)]
    #[case::above_max(951)]
    fn test_split_ratio_new_when_value_is_outside_bounds_returns_error(#[case] value: u16) {
        assert_that!(SplitRatio::new(value), err(anything()));
    }

    #[rstest::rstest]
    #[case::min(50)]
    #[case::current_vertical_default(400)]
    #[case::current_horizontal_default(500)]
    #[case::max(950)]
    fn test_split_ratio_new_when_value_is_inside_bounds_returns_ratio(#[case] value: u16) -> rootcause::Result<()> {
        assert_that!(SplitRatio::new(value)?.per_mille(), eq(value));
        Ok(())
    }

    #[rstest::rstest]
    #[case::zero(0)]
    #[case::above_max(951)]
    fn test_split_resize_step_new_when_value_is_outside_bounds_returns_error(#[case] value: u16) {
        assert_that!(SplitResizeStep::new(value), err(anything()));
    }
}
