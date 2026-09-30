//! Pane borders, attention highlighting, and unfocused dimming.

use muxr_core::RenderColor;

/// Pane border styles by semantic border role.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PaneBorderStyles {
    pub default: CellStyle,
    pub focused: CellStyle,
    pub resize: CellStyle,
}

impl Default for PaneBorderStyles {
    fn default() -> Self {
        Self {
            default: CellStyle {
                attrs: TextAttrs { bold: false },
                bg: RenderColor::Default,
                fg: RenderColor::Rgb { r: 50, g: 50, b: 50 },
            },
            focused: CellStyle {
                attrs: TextAttrs { bold: false },
                bg: RenderColor::Default,
                fg: RenderColor::Rgb { r: 50, g: 50, b: 50 },
            },
            resize: CellStyle {
                attrs: TextAttrs { bold: true },
                bg: RenderColor::Default,
                fg: RenderColor::Rgb { r: 106, g: 106, b: 223 },
            },
        }
    }
}

/// Pane attention styling.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PaneAttentionConfig {
    pub border: CellStyle,
    pub bg_tint: Option<RenderColor>,
}

impl Default for PaneAttentionConfig {
    fn default() -> Self {
        Self {
            border: CellStyle {
                attrs: TextAttrs { bold: false },
                bg: RenderColor::Default,
                fg: RenderColor::Rgb { r: 50, g: 50, b: 50 },
            },
            bg_tint: Some(RenderColor::Rgb { r: 32, g: 0, b: 0 }),
        }
    }
}

/// Unfocused pane dimming config.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PaneDimConfig {
    pub explicit_color_percent: u8,
    pub unfocused: bool,
}

impl Default for PaneDimConfig {
    fn default() -> Self {
        Self {
            explicit_color_percent: 80,
            unfocused: true,
        }
    }
}

/// Terminal cell style independent from any renderer backend.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CellStyle {
    pub attrs: TextAttrs,
    pub bg: RenderColor,
    pub fg: RenderColor,
}

/// Terminal text attributes used by configured muxr-owned UI cells.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TextAttrs {
    pub bold: bool,
}
