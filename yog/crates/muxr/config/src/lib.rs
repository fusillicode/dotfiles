//! Hardcoded muxr configuration.
//!
//! This crate owns static policy and tuning knobs. Runtime state, PTY observation, rendering algorithms, and protocol
//! transport stay in their feature crates. Colors are intentionally semantic config values; feature tests should assert
//! roles such as focused, resize, attention, or selected instead of concrete color values.

use muxr_core::RenderColor;

pub use self::keybindings::KeybindingAction;
pub use self::keybindings::KeybindingMode;
pub use self::keybindings::KeybindingsConfig;
pub use self::keybindings::LocalKeybindingAction;
pub use self::session_layout::ExternalLayoutPane;
pub use self::session_layout::ExternalLayoutTab;
pub use self::session_layout::ExternalSessionLayout;
pub use self::tracked_process::ProcessMatcher;
pub use self::tracked_process::ScreenObservationConfig;
pub use self::tracked_process::TrackedProcess;
pub use self::tracked_process::TrackedProcessId;

mod keybindings;
mod session_layout;
mod tracked_process;

pub const SPLIT_RATIO_MIN_PER_MILLE: u16 = 50;
pub const SPLIT_RATIO_MAX_PER_MILLE: u16 = 950;
const SPLIT_RESIZE_STEP_MIN: u16 = 1;
const SPLIT_RESIZE_STEP_MAX: u16 = 950;

/// Full hardcoded muxr config.
#[derive(Clone, Debug)]
pub struct MuxrConfig {
    /// These tables are compiled into both muxr binaries. Edit the default in `keybindings.rs` and rebuild muxr to
    /// change a key.
    pub keybindings: KeybindingsConfig,
    pub layout: LayoutConfig,
    pub pane_attention: PaneAttentionConfig,
    pub pane_borders: PaneBorderStyles,
    pub pane_dim: PaneDimConfig,
    pub scrollback: ScrollbackConfig,
    pub selection: SelectionStyle,
    pub tab_bar: TabBarConfig,
    pub tracked_processes: Vec<TrackedProcess>,
}

impl MuxrConfig {
    /// Build the static configuration and compile its screen regexes.
    ///
    /// # Errors
    /// Returns an error if a configured screen regex is invalid.
    pub fn new() -> rootcause::Result<Self> {
        Ok(Self {
            keybindings: KeybindingsConfig::default(),
            layout: LayoutConfig {
                horizontal_split_ratio: SplitRatio(500),
                resize_step: SplitResizeStep(50),
                vertical_split_ratio: SplitRatio(400),
            },
            pane_attention: PaneAttentionConfig {
                border: CellStyle {
                    attrs: TextAttrs { bold: false },
                    bg: RenderColor::Default,
                    fg: RenderColor::Rgb { r: 50, g: 50, b: 50 },
                },
                bg_tint: Some(RenderColor::Rgb { r: 32, g: 0, b: 0 }),
            },
            pane_borders: PaneBorderStyles {
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
            },
            pane_dim: PaneDimConfig {
                explicit_color_percent: 80,
                unfocused: true,
            },
            scrollback: ScrollbackConfig {
                dump_style: ScrollbackDumpStyle::PlainText,
                editor: ScrollbackEditorConfig {
                    program: "nvim",
                    // Scrollback opens in a read-only, no-swap nvim profile at the bottom of the dump; Esc quits the
                    // temporary viewer so it behaves like a muxr mode instead of a normal editor session.
                    args: &[
                        "-u",
                        "~/.config/nvim/minimal.lua",
                        "-R",
                        "-n",
                        "+",
                        "-c",
                        "nnoremap <buffer> <silent> <Esc> :quit!<CR>",
                    ],
                },
                rows: 50_000,
            },
            selection: SelectionStyle {
                bg: RenderColor::Indexed(238),
            },
            tab_bar: TabBarConfig {
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
            },
            tracked_processes: tracked_process::defaults()?,
        })
    }

    /// Return the first configured process matching a foreground executable and optional path.
    pub fn tracked_process_for_cmd(&self, executable: &str, path: Option<&str>) -> Option<&TrackedProcess> {
        self.tracked_processes
            .iter()
            .find(|process| process.matches(executable, path))
    }
}

/// Pane layout tuning.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LayoutConfig {
    pub horizontal_split_ratio: SplitRatio,
    pub resize_step: SplitResizeStep,
    pub vertical_split_ratio: SplitRatio,
}

/// A pane split ratio expressed in parts per thousand.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SplitRatio(u16);

impl SplitRatio {
    /// Build a split ratio in parts per thousand.
    ///
    /// # Errors
    /// Returns an error when `value` is outside the range supported by muxr pane layout.
    pub fn new(value: u16) -> rootcause::Result<Self> {
        if !(SPLIT_RATIO_MIN_PER_MILLE..=SPLIT_RATIO_MAX_PER_MILLE).contains(&value) {
            return Err(rootcause::report!("muxr split ratio is outside supported bounds")
                .attach(format!("min={SPLIT_RATIO_MIN_PER_MILLE}"))
                .attach(format!("max={SPLIT_RATIO_MAX_PER_MILLE}"))
                .attach(format!("actual={value}")));
        }
        Ok(Self(value))
    }

    /// Return the split ratio in parts per thousand.
    pub const fn per_mille(self) -> u16 {
        self.0
    }
}

/// A pane split resize delta expressed in parts per thousand.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SplitResizeStep(u16);

impl SplitResizeStep {
    /// Build a split resize delta in parts per thousand.
    ///
    /// # Errors
    /// Returns an error when `value` is zero or larger than the supported split-ratio range.
    pub fn new(value: u16) -> rootcause::Result<Self> {
        if !(SPLIT_RESIZE_STEP_MIN..=SPLIT_RESIZE_STEP_MAX).contains(&value) {
            return Err(rootcause::report!("muxr split resize step is outside supported bounds")
                .attach(format!("min={SPLIT_RESIZE_STEP_MIN}"))
                .attach(format!("max={SPLIT_RESIZE_STEP_MAX}"))
                .attach(format!("actual={value}")));
        }
        Ok(Self(value))
    }

    /// Return the resize step in parts per thousand.
    pub const fn per_mille(self) -> u16 {
        self.0
    }
}

/// Pane border styles by semantic border role.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PaneBorderStyles {
    pub default: CellStyle,
    pub focused: CellStyle,
    pub resize: CellStyle,
}

/// Pane attention styling.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PaneAttentionConfig {
    pub border: CellStyle,
    pub bg_tint: Option<RenderColor>,
}

/// Unfocused pane dimming config.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PaneDimConfig {
    pub explicit_color_percent: u8,
    pub unfocused: bool,
}

/// Terminal scrollback config.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScrollbackConfig {
    /// Dump format used by the server-owned scrollback viewer keybinding.
    pub dump_style: ScrollbackDumpStyle,
    /// External editor used by the scrollback viewer.
    pub editor: ScrollbackEditorConfig,
    /// Number of rows retained for each server-side terminal scrollback source.
    pub rows: usize,
}

/// Dump format used by the server-owned scrollback viewer keybinding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScrollbackDumpStyle {
    PlainText,
    Ansi,
}

/// External editor used by the scrollback viewer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScrollbackEditorConfig {
    pub program: &'static str,
    /// Args passed before the generated scrollback dump path.
    ///
    /// Args starting with `~/` are expanded against `$HOME` before spawning the editor.
    pub args: &'static [&'static str],
}

/// Muxr-owned selection styling.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SelectionStyle {
    pub bg: RenderColor,
}

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

    #[test]
    fn test_muxr_config_when_constructed_contains_valid_layout_values() -> rootcause::Result<()> {
        let config = MuxrConfig::new()?;

        SplitRatio::new(config.layout.horizontal_split_ratio.per_mille())?;
        SplitRatio::new(config.layout.vertical_split_ratio.per_mille())?;
        SplitResizeStep::new(config.layout.resize_step.per_mille())?;
        Ok(())
    }

    #[test]
    fn test_config_when_constructed_exposes_semantic_roles() {
        let config = MuxrConfig::new().unwrap();

        assert_that!(config.pane_borders.focused, eq(config.pane_borders.default));
        assert_that!(config.pane_attention.border, eq(config.pane_borders.default));
        assert_that!(config.pane_borders.resize.attrs.bold, eq(true));
        assert_that!(config.pane_attention.bg_tint, some(anything()));
        assert_that!(config.pane_dim.unfocused, eq(true));
        assert_that!(config.pane_dim.explicit_color_percent, gt(0));
        assert_that!(config.pane_dim.explicit_color_percent, le(100));
        assert_that!(config.scrollback.editor.args, points_to(each(not(eq("")))));
        assert_that!(config.scrollback.editor.program, not(eq("")));
        assert_that!(config.tab_bar.width, gt(0));
    }
}
