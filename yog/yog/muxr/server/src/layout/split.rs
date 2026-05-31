use rootcause::prelude::ResultExt;
use rootcause::report;
use serde::Deserialize;
use serde::Deserializer;

use crate::layout::DEFAULT_SPLIT_RATIO;
use crate::layout::MAX_SPLIT_RATIO;
use crate::layout::MIN_SPLIT_RATIO;
use crate::layout::PaneResizeDirection;
use crate::layout::PaneSplitAxis;
use crate::layout::PaneSplitRatio;
use crate::layout::PaneSplitResize;
use crate::layout::SPLIT_RATIO_HALF_SCALE;
use crate::layout::SPLIT_RATIO_SCALE;
use crate::layout::SPLIT_RESIZE_STEP;

impl PaneSplitRatio {
    pub const fn balanced() -> Self {
        Self(DEFAULT_SPLIT_RATIO)
    }

    pub fn new(value: u16) -> rootcause::Result<Self> {
        if !(MIN_SPLIT_RATIO..=MAX_SPLIT_RATIO).contains(&value) {
            return Err(report!("muxr pane split ratio is outside supported bounds")
                .attach(format!("min={MIN_SPLIT_RATIO}"))
                .attach(format!("max={MAX_SPLIT_RATIO}"))
                .attach(format!("actual={value}")));
        }
        Ok(Self(value))
    }

    pub fn resized(self, resize: PaneSplitResize) -> Self {
        match resize {
            PaneSplitResize::DecreaseFirst => Self(self.0.saturating_sub(SPLIT_RESIZE_STEP).max(MIN_SPLIT_RATIO)),
            PaneSplitResize::IncreaseFirst => Self(self.0.saturating_add(SPLIT_RESIZE_STEP).min(MAX_SPLIT_RATIO)),
        }
    }

    pub fn split_lengths(self, total: u16) -> rootcause::Result<(u16, u16)> {
        if total < 2 {
            return Err(report!("muxr terminal is too small for pane split").attach(format!("cells={total}")));
        }
        let max_first = total
            .checked_sub(1)
            .ok_or_else(|| report!("muxr pane split max length underflowed"))?;

        let scaled = u32::from(total)
            .checked_mul(u32::from(self.0))
            .ok_or_else(|| report!("muxr pane split ratio multiplication overflowed"))?;
        let rounded = scaled
            .checked_add(u32::from(SPLIT_RATIO_HALF_SCALE))
            .ok_or_else(|| report!("muxr pane split ratio rounding overflowed"))?;
        let first = rounded
            .checked_div(u32::from(SPLIT_RATIO_SCALE))
            .ok_or_else(|| report!("muxr pane split ratio divisor was zero"))?
            .clamp(1, u32::from(max_first));
        let first = u16::try_from(first).context("muxr pane split ratio result overflowed")?;
        let second = total
            .checked_sub(first)
            .ok_or_else(|| report!("muxr pane split ratio produced an invalid second length"))?;

        Ok((first, second))
    }
}

impl<'de> Deserialize<'de> for PaneSplitRatio {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u16::deserialize(deserializer)?;
        Self::new(value).map_err(|error| serde::de::Error::custom(format!("{error:#}")))
    }
}

impl PaneSplitResize {
    pub const fn for_direction(axis: PaneSplitAxis, direction: PaneResizeDirection) -> Option<Self> {
        match (axis, direction) {
            (PaneSplitAxis::Horizontal, PaneResizeDirection::Up)
            | (PaneSplitAxis::Vertical, PaneResizeDirection::Left) => Some(Self::DecreaseFirst),
            (PaneSplitAxis::Horizontal, PaneResizeDirection::Down)
            | (PaneSplitAxis::Vertical, PaneResizeDirection::Right) => Some(Self::IncreaseFirst),
            (PaneSplitAxis::Horizontal, PaneResizeDirection::Left | PaneResizeDirection::Right)
            | (PaneSplitAxis::Vertical, PaneResizeDirection::Down | PaneResizeDirection::Up) => None,
        }
    }
}
