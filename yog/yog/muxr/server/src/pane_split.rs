use std::sync::Mutex;

use muxr_core::PaneId;
use muxr_core::TerminalSize;
use rootcause::prelude::ResultExt;
use rootcause::report;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;

use crate::server::PaneRuntimes;
use crate::server::ServerConfig;
use crate::state::Pane;
use crate::state::PaneAttentionState;
use crate::state::PaneState;
use crate::state::PaneTree;
use crate::state::SessionLayout;
use crate::state::SessionMetadata;
use crate::state::Tab;

const SPLIT_RATIO_SCALE: u16 = 1000;
const SPLIT_RATIO_HALF_SCALE: u16 = 500;
const DEFAULT_HORIZONTAL_SPLIT_RATIO: u16 = 500;
const DEFAULT_VERTICAL_SPLIT_RATIO: u16 = 400;
const MIN_SPLIT_RATIO: u16 = 50;
const MAX_SPLIT_RATIO: u16 = 950;
const SPLIT_RESIZE_STEP: u16 = 50;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum PaneSplitAxis {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct PaneSplitRatio(u16);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaneSplitResize {
    DecreaseFirst,
    IncreaseFirst,
}

impl PaneSplitRatio {
    const fn default_for_axis(axis: PaneSplitAxis) -> Self {
        match axis {
            PaneSplitAxis::Horizontal => Self(DEFAULT_HORIZONTAL_SPLIT_RATIO),
            PaneSplitAxis::Vertical => Self(DEFAULT_VERTICAL_SPLIT_RATIO),
        }
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

impl SessionLayout {
    pub fn split_active_pane(
        &mut self,
        metadata: SessionMetadata,
        split_axis: PaneSplitAxis,
    ) -> rootcause::Result<PaneId> {
        let pane_id = PaneId::new(self.next_pane_number()?)?;
        let tab = self.active_tab_mut()?;
        let focus_seq = tab.next_focus_seq()?;
        let new_pane = Pane {
            attention_state: PaneAttentionState::Idle,
            cmd_label: metadata.cmd_label.clone(),
            cwd: metadata.cwd,
            focus_seq,
            id: pane_id,
            started_at: metadata.started_at,
            state: PaneState::Running,
            title: metadata.cmd_label,
        };
        tab.split_active_pane(&new_pane, split_axis)?;
        tab.active_pane = pane_id;
        Ok(pane_id)
    }
}

impl Tab {
    pub fn split_active_pane(&mut self, new_pane: &Pane, split_axis: PaneSplitAxis) -> rootcause::Result<()> {
        if !self.pane_tree.split_pane(self.active_pane, new_pane, split_axis)? {
            return Err(report!("muxr active pane is missing from server layout")
                .attach(format!("active_pane={}", self.active_pane)));
        }
        Ok(())
    }
}

impl PaneTree {
    pub fn split_pane(
        &mut self,
        pane_id: PaneId,
        new_pane: &Pane,
        split_axis: PaneSplitAxis,
    ) -> rootcause::Result<bool> {
        match self {
            Self::Pane(pane) if pane.id == pane_id => {
                let old_pane = pane.clone();
                *self = Self::Split {
                    axis: split_axis,
                    first_ratio: PaneSplitRatio::default_for_axis(split_axis),
                    first: Box::new(Self::Pane(old_pane)),
                    second: Box::new(Self::Pane(new_pane.clone())),
                };
                Ok(true)
            }
            Self::Pane(_) => Ok(false),
            Self::Split { first, second, .. } => {
                if first.split_pane(pane_id, new_pane, split_axis)? {
                    return Ok(true);
                }
                second.split_pane(pane_id, new_pane, split_axis)
            }
        }
    }
}

pub fn handle_split_pane_cmd(
    split_axis: PaneSplitAxis,
    config: &ServerConfig,
    layout: &Mutex<SessionLayout>,
    runtimes: &Mutex<PaneRuntimes>,
    terminal_size: &TerminalSize,
) -> rootcause::Result<PaneId> {
    let mut layout = crate::server::lock_mutex(layout, "layout")?;
    crate::server::sync_layout_terminal_titles(&mut layout, runtimes)?;
    let metadata = crate::server::active_pane_session_metadata(config, &layout)?;
    let previous_layout = layout.clone();
    let pane_id = layout.split_active_pane(metadata, split_axis)?;
    let pane_id = crate::server::spawn_pane_or_restore_layout(
        &mut layout,
        previous_layout,
        pane_id,
        config,
        runtimes,
        terminal_size,
    )?;
    crate::state::persisted::write_metadata(&config.paths, &layout)?;
    drop(layout);
    Ok(pane_id)
}
