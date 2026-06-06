use std::sync::Mutex;

use muxr_config::LayoutConfig;

use crate::pane_split::PaneSplitAxis;
use crate::pane_split::PaneSplitResize;
use crate::server::ServerConfig;
use crate::state::PaneTree;
use crate::state::SessionLayout;
use crate::state::Tab;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaneResizeDirection {
    Down,
    Left,
    Right,
    Up,
}

impl SessionLayout {
    pub fn resize_active_pane(
        &mut self,
        layout_config: LayoutConfig,
        direction: PaneResizeDirection,
    ) -> rootcause::Result<bool> {
        self.active_tab_mut()?.resize_active_pane(layout_config, direction)
    }
}

impl Tab {
    pub fn resize_active_pane(
        &mut self,
        layout_config: LayoutConfig,
        direction: PaneResizeDirection,
    ) -> rootcause::Result<bool> {
        self.pane_tree.resize_pane(layout_config, self.active_pane, direction)
    }
}

impl PaneTree {
    pub fn resize_pane(
        &mut self,
        layout_config: LayoutConfig,
        pane_id: muxr_core::PaneId,
        direction: PaneResizeDirection,
    ) -> rootcause::Result<bool> {
        match self {
            Self::Pane(_) => Ok(false),
            Self::Split {
                axis,
                first_ratio,
                first,
                second,
            } => {
                let child_resized = if first.contains_pane(pane_id) {
                    first.resize_pane(layout_config, pane_id, direction)?
                } else if second.contains_pane(pane_id) {
                    second.resize_pane(layout_config, pane_id, direction)?
                } else {
                    return Ok(false);
                };
                if child_resized {
                    return Ok(true);
                }

                let Some(resize) = PaneSplitResize::for_direction(*axis, direction) else {
                    return Ok(false);
                };
                let resized_ratio = first_ratio.resized(layout_config, resize)?;
                if resized_ratio == *first_ratio {
                    return Ok(false);
                }

                *first_ratio = resized_ratio;
                Ok(true)
            }
        }
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

pub fn handle_resize_pane_cmd(
    direction: PaneResizeDirection,
    config: &ServerConfig,
    layout: &Mutex<SessionLayout>,
) -> rootcause::Result<bool> {
    let mut layout = crate::server::lock_mutex(layout, "layout")?;
    let resized = layout.resize_active_pane(config.user_config.layout, direction)?;
    if resized {
        crate::state::persisted::write_metadata(&config.paths, &layout)?;
    }
    drop(layout);
    Ok(resized)
}

#[cfg(test)]
mod tests {
    use muxr_config::MuxrConfig;
    use muxr_core::TerminalSize;

    use super::*;
    use crate::state::test_helpers as state_test_helpers;

    #[rstest::rstest]
    #[case::vertical_left(
        PaneSplitAxis::Vertical,
        PaneResizeDirection::Left,
        vec![
            ("pane-1", 0, 0, 36, 24),
            ("pane-2", 37, 0, 43, 24),
        ],
    )]
    #[case::vertical_right(
        PaneSplitAxis::Vertical,
        PaneResizeDirection::Right,
        vec![
            ("pane-1", 0, 0, 43, 24),
            ("pane-2", 44, 0, 36, 24),
        ],
    )]
    #[case::horizontal_up(
        PaneSplitAxis::Horizontal,
        PaneResizeDirection::Up,
        vec![
            ("pane-1", 0, 0, 80, 10),
            ("pane-2", 0, 11, 80, 13),
        ],
    )]
    #[case::horizontal_down(
        PaneSplitAxis::Horizontal,
        PaneResizeDirection::Down,
        vec![
            ("pane-1", 0, 0, 80, 13),
            ("pane-2", 0, 14, 80, 10),
        ],
    )]
    fn test_layout_resize_active_pane_when_resize_cmd_arrives_updates_geometry(
        #[case] split_axis: PaneSplitAxis,
        #[case] direction: PaneResizeDirection,
        #[case] expected_regions: Vec<(&str, u16, u16, u16, u16)>,
    ) -> rootcause::Result<()> {
        let mut layout = state_test_helpers::layout("work")?;

        layout.split_active_pane(
            MuxrConfig::default().layout,
            state_test_helpers::metadata("sh", 2),
            split_axis,
        )?;
        state_test_helpers::force_balanced_test_split_ratio(&mut layout)?;

        assert2::assert!(layout.resize_active_pane(MuxrConfig::default().layout, direction)?);
        let expected_regions = expected_regions
            .into_iter()
            .map(|(id, col, row, cols, rows)| (id.to_owned(), col, row, cols, rows))
            .collect::<Vec<_>>();
        pretty_assertions::assert_eq!(
            state_test_helpers::layout_active_tab_pane_regions(&layout, &TerminalSize::new(80, 24)?)?,
            expected_regions
        );
        Ok(())
    }

    #[test]
    fn test_layout_resize_nested_splits_resizes_nearest_matching_axis() -> rootcause::Result<()> {
        let mut layout = state_test_helpers::layout("work")?;

        layout.split_active_pane(
            MuxrConfig::default().layout,
            state_test_helpers::metadata("sh", 2),
            PaneSplitAxis::Vertical,
        )?;
        layout.split_active_pane(
            MuxrConfig::default().layout,
            state_test_helpers::metadata("sh", 3),
            PaneSplitAxis::Horizontal,
        )?;
        state_test_helpers::force_balanced_test_split_ratio(&mut layout)?;

        assert2::assert!(layout.resize_active_pane(MuxrConfig::default().layout, PaneResizeDirection::Up)?);
        pretty_assertions::assert_eq!(
            state_test_helpers::layout_active_tab_pane_regions(&layout, &TerminalSize::new(80, 24)?)?,
            vec![
                ("pane-1".to_owned(), 0, 0, 40, 24),
                ("pane-2".to_owned(), 41, 0, 39, 10),
                ("pane-3".to_owned(), 41, 11, 39, 13),
            ],
        );

        assert2::assert!(layout.resize_active_pane(MuxrConfig::default().layout, PaneResizeDirection::Left)?);
        pretty_assertions::assert_eq!(
            state_test_helpers::layout_active_tab_pane_regions(&layout, &TerminalSize::new(80, 24)?)?,
            vec![
                ("pane-1".to_owned(), 0, 0, 36, 24),
                ("pane-2".to_owned(), 37, 0, 43, 10),
                ("pane-3".to_owned(), 37, 11, 43, 13),
            ],
        );
        Ok(())
    }
}
