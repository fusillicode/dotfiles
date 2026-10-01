//! Hardcoded muxr configuration.
//!
//! This crate owns static policy and tuning knobs. Runtime state, PTY observation, rendering algorithms, and protocol
//! transport stay in their feature crates. Colors are intentionally semantic config values; feature tests should assert
//! roles such as focused, resize, attention, or selected instead of concrete color values.

pub use self::keybindings::KeybindingAction;
pub use self::keybindings::KeybindingMode;
pub use self::keybindings::KeybindingsConfig;
pub use self::keybindings::LocalKeybindingAction;
pub use self::layout::LayoutConfig;
pub use self::layout::SPLIT_RATIO_MAX_PER_MILLE;
pub use self::layout::SPLIT_RATIO_MIN_PER_MILLE;
pub use self::layout::SplitRatio;
pub use self::layout::SplitResizeStep;
pub use self::pane_appearance::CellStyle;
pub use self::pane_appearance::PaneAttentionConfig;
pub use self::pane_appearance::PaneBorderStyles;
pub use self::pane_appearance::PaneDimConfig;
pub use self::pane_appearance::TextAttrs;
pub use self::scrollback::ScrollbackConfig;
pub use self::scrollback::ScrollbackDumpStyle;
pub use self::scrollback::ScrollbackEditorConfig;
pub use self::selection::SelectionStyle;
pub use self::session_layout::ExternalLayoutPane;
pub use self::session_layout::ExternalLayoutTab;
pub use self::session_layout::ExternalSessionLayout;
pub use self::tab_bar::RailStyle;
pub use self::tab_bar::TabBarConfig;
pub use self::tab_bar::TrackedProcessStyle;
pub use self::tracked_process::ObservationPatterns;
pub use self::tracked_process::ProcessMatcher;
pub use self::tracked_process::ScreenObservationConfig;
pub use self::tracked_process::TrackedProcess;
pub use self::tracked_process::TrackedProcessConfig;
pub use self::tracked_process::TrackedProcessId;

mod keybindings;
mod layout;
mod pane_appearance;
mod scrollback;
mod selection;
mod session_layout;
mod tab_bar;
mod tracked_process;

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
    pub tracked_processes: TrackedProcessConfig,
}

impl MuxrConfig {
    /// Build the static configuration and compile its screen regexes.
    ///
    /// # Errors
    /// Returns an error if a regex, split value, or keybinding character is invalid, or an observation pattern list is
    /// empty.
    pub fn new() -> rootcause::Result<Self> {
        Ok(Self {
            keybindings: KeybindingsConfig::new()?,
            layout: LayoutConfig::new()?,
            pane_attention: PaneAttentionConfig::default(),
            pane_borders: PaneBorderStyles::default(),
            pane_dim: PaneDimConfig::default(),
            scrollback: ScrollbackConfig::default(),
            selection: SelectionStyle::default(),
            tab_bar: TabBarConfig::default(),
            tracked_processes: TrackedProcessConfig::new()?,
        })
    }

    /// Return the first configured process matching a foreground executable and optional path.
    pub fn tracked_process_for_cmd(&self, executable: &str, path: Option<&str>) -> Option<&TrackedProcess> {
        self.tracked_processes
            .processes
            .iter()
            .find(|process| process.matches(executable, path))
    }
}

#[cfg(test)]
mod tests {
    use test_that::prelude::*;

    use super::*;

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
