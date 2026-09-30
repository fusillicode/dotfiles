//! Sidebar width, colors, rail, and process indicators.

use muxr_core::RenderColor;

/// Left sidebar tab-bar config.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TabBarConfig {
    pub active_fg: RenderColor,
    pub bg: RenderColor,
    pub inactive_fg: RenderColor,
    pub rail: RailStyle,
    pub separator_fg: RenderColor,
    pub tracked_process: TrackedProcessStyle,
    pub width: u16,
}

impl Default for TabBarConfig {
    fn default() -> Self {
        Self {
            active_fg: RenderColor::Indexed(7),
            bg: RenderColor::Rgb { r: 0, g: 19, b: 0 },
            inactive_fg: RenderColor::Rgb { r: 119, g: 119, b: 119 },
            rail: RailStyle {
                active_fg: RenderColor::Rgb { r: 106, g: 106, b: 223 },
                inactive_fg: RenderColor::Rgb { r: 0, g: 19, b: 0 },
            },
            separator_fg: RenderColor::Rgb { r: 50, g: 50, b: 50 },
            tracked_process: TrackedProcessStyle {
                busy_fg: RenderColor::Rgb { r: 140, g: 228, b: 121 },
                unseen_fg: RenderColor::Rgb { r: 255, g: 0, b: 0 },
            },
            width: 21,
        }
    }
}

/// Tab-bar rail styling.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RailStyle {
    pub active_fg: RenderColor,
    pub inactive_fg: RenderColor,
}

/// Tab-bar tracked-process marker styling.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TrackedProcessStyle {
    pub busy_fg: RenderColor,
    pub unseen_fg: RenderColor,
}
