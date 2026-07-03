use muxr_config::LayoutConfig;
use muxr_config::SPLIT_RATIO_MAX_PER_MILLE;
use muxr_config::SPLIT_RATIO_MIN_PER_MILLE;
use muxr_core::PaneId;
use muxr_core::TerminalSize;
use rootcause::prelude::ResultExt;
use rootcause::report;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;

use crate::client::session::ClientSessionState;
use crate::pane::runtime::PaneRuntimes;
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PaneSplitClientOutcome {
    pub new_pane_id: PaneId,
    pub previous_pane: PaneId,
}

impl PaneSplitRatio {
    fn default_for_axis(layout_config: LayoutConfig, axis: PaneSplitAxis) -> rootcause::Result<Self> {
        let ratio = match axis {
            PaneSplitAxis::Horizontal => layout_config.horizontal_split_ratio,
            PaneSplitAxis::Vertical => layout_config.vertical_split_ratio,
        };
        Self::new(ratio.per_mille())
    }

    pub fn new(value: u16) -> rootcause::Result<Self> {
        muxr_config::SplitRatio::new(value)?;
        Ok(Self(value))
    }

    pub fn resized(self, layout_config: LayoutConfig, resize: PaneSplitResize) -> rootcause::Result<Self> {
        let resize_step = layout_config.resize_step.per_mille();
        let value = match resize {
            PaneSplitResize::DecreaseFirst => self.0.saturating_sub(resize_step).max(SPLIT_RATIO_MIN_PER_MILLE),
            PaneSplitResize::IncreaseFirst => self.0.saturating_add(resize_step).min(SPLIT_RATIO_MAX_PER_MILLE),
        };
        Self::new(value)
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
        layout_config: LayoutConfig,
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
        tab.split_active_pane(layout_config, &new_pane, split_axis)?;
        tab.active_pane = pane_id;
        Ok(pane_id)
    }
}

impl Tab {
    pub fn split_active_pane(
        &mut self,
        layout_config: LayoutConfig,
        new_pane: &Pane,
        split_axis: PaneSplitAxis,
    ) -> rootcause::Result<()> {
        if self
            .pane_tree
            .split_pane(layout_config, self.active_pane, new_pane, split_axis)?
            == PaneSplit::Missing
        {
            return Err(report!("muxr active pane is missing from server layout")
                .attach(format!("active_pane={}", self.active_pane)));
        }
        Ok(())
    }
}

impl PaneTree {
    pub fn split_pane(
        &mut self,
        layout_config: LayoutConfig,
        pane_id: PaneId,
        new_pane: &Pane,
        split_axis: PaneSplitAxis,
    ) -> rootcause::Result<PaneSplit> {
        match self {
            Self::Pane(pane) if pane.id == pane_id => {
                let old_pane = pane.clone();
                *self = Self::Split {
                    axis: split_axis,
                    first_ratio: PaneSplitRatio::default_for_axis(layout_config, split_axis)?,
                    first: Box::new(Self::Pane(old_pane)),
                    second: Box::new(Self::Pane(new_pane.clone())),
                };
                Ok(PaneSplit::Split)
            }
            Self::Pane(_) => Ok(PaneSplit::Missing),
            Self::Split { first, second, .. } => {
                if first.split_pane(layout_config, pane_id, new_pane, split_axis)? == PaneSplit::Split {
                    return Ok(PaneSplit::Split);
                }
                second.split_pane(layout_config, pane_id, new_pane, split_axis)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaneSplit {
    Missing,
    Split,
}

fn handle_split_pane_cmd(
    split_axis: PaneSplitAxis,
    config: &ServerConfig,
    layout: &mut SessionLayout,
    runtimes: &mut PaneRuntimes,
    terminal_size: &TerminalSize,
) -> rootcause::Result<PaneId> {
    let _synced = runtimes.sync_layout_terminal_titles(layout);
    let metadata = crate::server::active_pane_session_metadata(config, layout)?;
    let previous_layout = layout.clone();
    let pane_id = layout.split_active_pane(config.user_config.layout, metadata, split_axis)?;
    let pane_id = crate::pane::runtime::spawn_pane_or_restore_layout(
        layout,
        previous_layout,
        pane_id,
        config,
        runtimes,
        terminal_size,
    )?;
    crate::state::persisted::write_metadata(&config.paths, layout)?;
    Ok(pane_id)
}

pub fn handle_split_pane_cmd_client(
    split_axis: PaneSplitAxis,
    state: &mut ClientSessionState<'_>,
) -> rootcause::Result<PaneSplitClientOutcome> {
    let previous_pane = state.layout.active_pane_id()?;
    crate::pane::fullscreen::clear_active_tab_for_layout_mutation(state);
    let new_pane_id = self::handle_split_pane_cmd(
        split_axis,
        state.config,
        state.layout,
        state.runtimes,
        &state.terminal_size,
    )?;
    Ok(PaneSplitClientOutcome {
        new_pane_id,
        previous_pane,
    })
}

#[cfg(test)]
mod tests {
    use muxr_config::MuxrConfig;
    use muxr_core::TerminalSize;
    use test_that::prelude::*;

    use super::*;
    use crate::pane::borders::PaneBorderAxis;
    use crate::pane::runtime::test_helpers as pane_runtime_test_helpers;
    use crate::server::test_helpers as server_test_helpers;
    use crate::state::test_helpers as state_test_helpers;

    #[rstest::rstest]
    #[case::vertical_then_horizontal(
        PaneSplitAxis::Vertical,
        PaneSplitAxis::Horizontal,
        vec![
            ("pane-1", 0, 0, 40, 24),
            ("pane-2", 41, 0, 39, 12),
            ("pane-3", 41, 13, 39, 11),
        ],
    )]
    #[case::horizontal_then_vertical(
        PaneSplitAxis::Horizontal,
        PaneSplitAxis::Vertical,
        vec![
            ("pane-1", 0, 0, 80, 12),
            ("pane-2", 0, 13, 40, 11),
            ("pane-3", 41, 13, 39, 11),
        ],
    )]
    fn test_layout_split_when_nested_splits_only_active_pane(
        #[case] first_axis: PaneSplitAxis,
        #[case] second_axis: PaneSplitAxis,
        #[case] expected_regions: Vec<(&str, u16, u16, u16, u16)>,
    ) -> rootcause::Result<()> {
        let mut layout = state_test_helpers::layout("work")?;

        layout.split_active_pane(
            MuxrConfig::default().layout,
            state_test_helpers::metadata("sh", 2),
            first_axis,
        )?;
        layout.split_active_pane(
            MuxrConfig::default().layout,
            state_test_helpers::metadata("sh", 3),
            second_axis,
        )?;
        state_test_helpers::force_balanced_test_split_ratio(&mut layout)?;

        assert_that!(layout.active_pane_id()?.to_string(), eq("pane-3"));
        assert_that!(
            state_test_helpers::layout_active_tab_pane_ids(&layout)?,
            eq(vec!["pane-1", "pane-2", "pane-3"])
        );
        let expected_regions = expected_regions
            .into_iter()
            .map(|(id, col, row, cols, rows)| (id.to_owned(), col, row, cols, rows))
            .collect::<Vec<_>>();
        assert_that!(
            state_test_helpers::layout_active_tab_pane_regions(&layout, &TerminalSize::new(80, 24)?)?,
            eq(expected_regions)
        );
        Ok(())
    }

    #[rstest::rstest]
    #[case::vertical(
        PaneSplitAxis::Vertical,
        vec![(PaneBorderAxis::Vertical, 40, 0, 24)],
    )]
    #[case::horizontal(
        PaneSplitAxis::Horizontal,
        vec![(PaneBorderAxis::Horizontal, 0, 12, 80)],
    )]
    fn test_layout_split_when_split_exists_reserves_border_cell(
        #[case] split_axis: PaneSplitAxis,
        #[case] expected_borders: Vec<(PaneBorderAxis, u16, u16, u16)>,
    ) -> rootcause::Result<()> {
        let mut layout = state_test_helpers::layout("work")?;

        layout.split_active_pane(
            MuxrConfig::default().layout,
            state_test_helpers::metadata("sh", 2),
            split_axis,
        )?;
        state_test_helpers::force_balanced_test_split_ratio(&mut layout)?;

        assert_that!(
            state_test_helpers::layout_active_tab_pane_borders(&layout, &TerminalSize::new(80, 24)?)?,
            eq(expected_borders)
        );
        Ok(())
    }

    #[test]
    fn test_handle_split_pane_cmd_when_pane_spawn_fails_restores_layout() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let mut config = server_test_helpers::server_config(tempdir.path(), "work")?;
        config.shell_cmd = server_test_helpers::shell_cmd("/bin/muxr-missing-shell");
        let initial_layout = SessionLayout::initial(&config.session, state_test_helpers::metadata("sh", 1))?;
        let mut layout = initial_layout.clone();
        let mut runtimes = pane_runtime_test_helpers::empty_runtimes();

        assert_that!(
            self::handle_split_pane_cmd(
                PaneSplitAxis::Vertical,
                &config,
                &mut layout,
                &mut runtimes,
                &TerminalSize::new(80, 24)?,
            ),
            err(anything())
        );

        assert_that!(layout, eq(initial_layout));
        assert_that!(
            runtimes.set_status(),
            eq(crate::pane::runtime::PaneRuntimeSetStatus::Empty)
        );
        assert_that!(config.paths.layout.exists(), eq(false));
        Ok(())
    }
}
