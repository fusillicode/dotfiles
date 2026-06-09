//! Hardcoded muxr configuration.
//!
//! This crate owns static policy and tuning knobs. Runtime state, PTY observation, rendering algorithms, and protocol
//! transport stay in their feature crates. Colors are intentionally semantic config values; feature tests should assert
//! roles such as focused, resize, attention, or selected instead of concrete color values.

use std::time::Duration;

use muxr_core::RenderColor;

pub use self::session_layout::ExternalLayoutPane;
pub use self::session_layout::ExternalLayoutTab;
pub use self::session_layout::ExternalSessionLayout;

mod session_layout;

pub const SPLIT_RATIO_MIN_PER_MILLE: u16 = 50;
pub const SPLIT_RATIO_MAX_PER_MILLE: u16 = 950;
const SPLIT_RESIZE_STEP_MIN: u16 = 1;
const SPLIT_RESIZE_STEP_MAX: u16 = 950;

/// Full hardcoded muxr config.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MuxrConfig {
    pub layout: LayoutConfig,
    pub pane_attention: PaneAttentionConfig,
    pub pane_borders: PaneBorderStyles,
    pub pane_dim: PaneDimConfig,
    /// Terminal scrollback retention.
    pub scrollback: ScrollbackConfig,
    pub selection: SelectionStyle,
    pub tab_bar: TabBarConfig,
    pub tracked_processes: Vec<TrackedProcess>,
}

impl Default for MuxrConfig {
    #[expect(
        clippy::too_many_lines,
        reason = "the hardcoded config stays in one visible literal so local tuning does not require jumping between helpers"
    )]
    fn default() -> Self {
        Self {
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
            scrollback: ScrollbackConfig { rows: 50_000 },
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
                width: 24,
            },
            tracked_processes: vec![
                TrackedProcess {
                    id: TrackedProcessId::Claude,
                    label: "cl",
                    matchers: vec![
                        ProcessMatcher::ExactExecutable("claude"),
                        ProcessMatcher::ExactExecutable("claude-code"),
                        ProcessMatcher::PathContains("/claude/versions/"),
                    ],
                    quiet_threshold: Duration::from_secs(3),
                },
                TrackedProcess {
                    id: TrackedProcessId::Codex,
                    label: "cx",
                    matchers: vec![
                        ProcessMatcher::ExactExecutable("codex"),
                        ProcessMatcher::ExactExecutable("codex-aarch64-apple-darwin"),
                        ProcessMatcher::ExactExecutable("codex-x86_64-apple-darwin"),
                    ],
                    quiet_threshold: Duration::from_secs(3),
                },
                TrackedProcess {
                    id: TrackedProcessId::Cursor,
                    label: "cu",
                    matchers: vec![
                        ProcessMatcher::ExactExecutable("cursor"),
                        ProcessMatcher::ExactExecutable("cursor-agent"),
                        ProcessMatcher::ExecutableWithPathContains {
                            executable: "node",
                            path_contains: "/cursor-agent/versions/",
                        },
                    ],
                    quiet_threshold: Duration::from_secs(3),
                },
                TrackedProcess {
                    id: TrackedProcessId::Gemini,
                    label: "gm",
                    matchers: vec![ProcessMatcher::ExactExecutable("gemini")],
                    quiet_threshold: Duration::from_secs(3),
                },
                TrackedProcess {
                    id: TrackedProcessId::Opencode,
                    label: "oc",
                    matchers: vec![ProcessMatcher::ExactExecutable("opencode")],
                    quiet_threshold: Duration::from_secs(3),
                },
            ],
        }
    }
}

impl MuxrConfig {
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

/// Terminal scrollback retention config.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScrollbackConfig {
    /// Number of rows retained for each server-side terminal scrollback source.
    pub rows: usize,
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

/// One foreground process class that can drive tab-bar dots and quiet-attention state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrackedProcess {
    pub id: TrackedProcessId,
    pub label: &'static str,
    pub matchers: Vec<ProcessMatcher>,
    pub quiet_threshold: Duration,
}

impl TrackedProcess {
    /// Return true when any matcher identifies this tracked process.
    pub fn matches(&self, executable: &str, path: Option<&str>) -> bool {
        self.matchers.iter().any(|matcher| matcher.matches(executable, path))
    }
}

/// Stable ids for initially configured tracked processes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrackedProcessId {
    Claude,
    Codex,
    Cursor,
    Gemini,
    Opencode,
}

/// A foreground process matcher.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProcessMatcher {
    ExactExecutable(&'static str),
    PathContains(&'static str),
    ExecutableWithPathContains {
        executable: &'static str,
        path_contains: &'static str,
    },
}

impl ProcessMatcher {
    /// Return true when this matcher identifies a foreground process.
    pub fn matches(self, executable: &str, path: Option<&str>) -> bool {
        match self {
            Self::ExactExecutable(expected) => executable == expected,
            Self::PathContains(needle) => path.is_some_and(|path| path.contains(needle)),
            Self::ExecutableWithPathContains {
                executable: expected,
                path_contains,
            } => executable == expected && path.is_some_and(|path| path.contains(path_contains)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[rstest::rstest]
    #[case::below_min(49)]
    #[case::above_max(951)]
    fn test_split_ratio_new_when_value_is_outside_bounds_returns_error(#[case] value: u16) {
        assert2::assert!(SplitRatio::new(value).is_err());
    }

    #[rstest::rstest]
    #[case::min(50)]
    #[case::current_vertical_default(400)]
    #[case::current_horizontal_default(500)]
    #[case::max(950)]
    fn test_split_ratio_new_when_value_is_inside_bounds_returns_ratio(#[case] value: u16) -> rootcause::Result<()> {
        pretty_assertions::assert_eq!(SplitRatio::new(value)?.per_mille(), value);
        Ok(())
    }

    #[rstest::rstest]
    #[case::zero(0)]
    #[case::above_max(951)]
    fn test_split_resize_step_new_when_value_is_outside_bounds_returns_error(#[case] value: u16) {
        assert2::assert!(SplitResizeStep::new(value).is_err());
    }

    #[test]
    fn test_muxr_config_default_contains_valid_layout_values() -> rootcause::Result<()> {
        let config = MuxrConfig::default();

        SplitRatio::new(config.layout.horizontal_split_ratio.per_mille())?;
        SplitRatio::new(config.layout.vertical_split_ratio.per_mille())?;
        SplitResizeStep::new(config.layout.resize_step.per_mille())?;
        Ok(())
    }

    #[rstest::rstest]
    #[case::claude("claude", None, TrackedProcessId::Claude, "cl")]
    #[case::claude_code("claude-code", None, TrackedProcessId::Claude, "cl")]
    #[case::claude_versioned_runtime(
        "node",
        Some("/Users/me/claude/versions/1.2.3/node"),
        TrackedProcessId::Claude,
        "cl"
    )]
    #[case::codex("codex", None, TrackedProcessId::Codex, "cx")]
    #[case::codex_aarch64("codex-aarch64-apple-darwin", None, TrackedProcessId::Codex, "cx")]
    #[case::cursor("cursor", None, TrackedProcessId::Cursor, "cu")]
    #[case::cursor_agent("cursor-agent", None, TrackedProcessId::Cursor, "cu")]
    #[case::cursor_versioned_runtime(
        "node",
        Some("/Users/me/cursor-agent/versions/1.2.3/node"),
        TrackedProcessId::Cursor,
        "cu"
    )]
    #[case::gemini("gemini", None, TrackedProcessId::Gemini, "gm")]
    #[case::opencode("opencode", None, TrackedProcessId::Opencode, "oc")]
    fn test_tracked_processes_when_command_matches_returns_process(
        #[case] executable: &str,
        #[case] path: Option<&str>,
        #[case] expected_id: TrackedProcessId,
        #[case] expected_label: &str,
    ) -> rootcause::Result<()> {
        let config = MuxrConfig::default();
        let process = config
            .tracked_process_for_cmd(executable, path)
            .ok_or_else(|| rootcause::report!("expected tracked process"))?;

        pretty_assertions::assert_eq!(process.id, expected_id);
        pretty_assertions::assert_eq!(process.label, expected_label);
        Ok(())
    }

    #[rstest::rstest]
    #[case::rg_codex("rg-codex", None)]
    #[case::notcodex("notcodex", None)]
    #[case::plain_node("node", None)]
    #[case::node_without_cursor_runtime("node", Some("/usr/local/bin/node"))]
    fn test_tracked_processes_when_command_does_not_match_returns_none(
        #[case] executable: &str,
        #[case] path: Option<&str>,
    ) {
        let config = MuxrConfig::default();

        pretty_assertions::assert_eq!(config.tracked_process_for_cmd(executable, path), None);
    }

    #[test]
    fn test_config_default_exposes_semantic_roles() {
        let config = MuxrConfig::default();

        pretty_assertions::assert_eq!(config.pane_borders.focused, config.pane_borders.default);
        pretty_assertions::assert_eq!(config.pane_attention.border, config.pane_borders.default);
        assert2::assert!(config.pane_borders.resize.attrs.bold);
        assert2::assert!(config.pane_attention.bg_tint.is_some());
        assert2::assert!(config.pane_dim.unfocused);
        assert2::assert!(config.pane_dim.explicit_color_percent > 0);
        assert2::assert!(config.pane_dim.explicit_color_percent <= 100);
        assert2::assert!(config.tab_bar.width > 0);
    }
}
