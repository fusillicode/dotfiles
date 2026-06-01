use std::sync::Mutex;

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
    pub fn resize_active_pane(&mut self, direction: PaneResizeDirection) -> rootcause::Result<bool> {
        self.active_tab_mut()?.resize_active_pane(direction)
    }
}

impl Tab {
    pub fn resize_active_pane(&mut self, direction: PaneResizeDirection) -> rootcause::Result<bool> {
        self.pane_tree.resize_pane(&self.active_pane, direction)
    }
}

impl PaneTree {
    pub fn resize_pane(
        &mut self,
        pane_id: &muxr_core::PaneId,
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
                    first.resize_pane(pane_id, direction)?
                } else if second.contains_pane(pane_id) {
                    second.resize_pane(pane_id, direction)?
                } else {
                    return Ok(false);
                };
                if child_resized {
                    return Ok(true);
                }

                let Some(resize) = PaneSplitResize::for_direction(*axis, direction) else {
                    return Ok(false);
                };
                let resized_ratio = first_ratio.resized(resize);
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

pub fn handle_resize_pane_command(
    direction: PaneResizeDirection,
    config: &ServerConfig,
    layout: &Mutex<SessionLayout>,
) -> rootcause::Result<bool> {
    let mut layout = crate::server::lock_mutex(layout, "layout")?;
    let resized = layout.resize_active_pane(direction)?;
    if resized {
        crate::state::persisted::write_metadata(&config.paths, &layout)?;
    }
    drop(layout);
    Ok(resized)
}
