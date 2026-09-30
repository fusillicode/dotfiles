//! Selection highlighting.

use muxr_core::RenderColor;

/// Muxr-owned selection styling.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SelectionStyle {
    pub bg: RenderColor,
}

impl Default for SelectionStyle {
    fn default() -> Self {
        Self {
            bg: RenderColor::Indexed(238),
        }
    }
}
