use muxr_core::RenderColor;
use muxr_core::RenderStyle;
use rootcause::report;

use crate::state::Pane;
use crate::state::PaneAttentionState;
use crate::state::SessionLayout;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PaneAttentionChange {
    Changed,
    #[default]
    Unchanged,
}

impl Pane {
    pub const fn acknowledge_attention(&mut self) -> PaneAttentionChange {
        self.clear_attention()
    }

    pub const fn clear_attention(&mut self) -> PaneAttentionChange {
        if !matches!(self.attention_state, PaneAttentionState::NeedsAttention) {
            return PaneAttentionChange::Unchanged;
        }
        self.attention_state = PaneAttentionState::Idle;
        PaneAttentionChange::Changed
    }
}

impl SessionLayout {
    pub fn acknowledge_active_pane_attention(&mut self) -> rootcause::Result<PaneAttentionChange> {
        let active_pane = self.active_pane_id()?;
        let Some(pane) = self.pane_mut(active_pane) else {
            return Err(
                report!("muxr active pane is missing from server layout").attach(format!("pane_id={active_pane}"))
            );
        };
        Ok(pane.acknowledge_attention())
    }

    pub fn attention_pane_ids(&self) -> Vec<muxr_core::PaneId> {
        // Attention is intentionally explicit. Raw PTY output is too broad because startup,
        // splits, and shell prompts would otherwise paint unfocused panes as needing attention.
        self.panes()
            .into_iter()
            .filter(|pane| pane.attention_state == PaneAttentionState::NeedsAttention)
            .map(|pane| pane.id)
            .collect()
    }
}

pub fn apply_attention_tint(mut style: RenderStyle, bg_tint: RenderColor) -> RenderStyle {
    style.bg = tinted_background(style.bg, bg_tint);
    style
}

fn tinted_background(background: RenderColor, tint: RenderColor) -> RenderColor {
    match (background, tint) {
        (
            RenderColor::Rgb { r, g, b },
            RenderColor::Rgb {
                r: tint_r,
                g: tint_g,
                b: tint_b,
            },
        ) => RenderColor::Rgb {
            r: blend_channel(r, tint_r),
            g: blend_channel(g, tint_g),
            b: blend_channel(b, tint_b),
        },
        (RenderColor::Default | RenderColor::Indexed(_) | RenderColor::Rgb { .. }, tint) => tint,
    }
}

fn blend_channel(base: u8, tint: u8) -> u8 {
    const BASE_WEIGHT: u16 = 3;
    const TINT_WEIGHT: u16 = 1;
    const TOTAL_WEIGHT: u16 = 4;

    let value = u16::from(base)
        .saturating_mul(BASE_WEIGHT)
        .saturating_add(u16::from(tint).saturating_mul(TINT_WEIGHT))
        .checked_div(TOTAL_WEIGHT)
        .unwrap_or_else(|| u16::from(u8::MAX));
    u8::try_from(value).unwrap_or(u8::MAX)
}

#[cfg(test)]
mod tests {
    use muxr_config::MuxrConfig;
    use muxr_core::PaneId;
    use muxr_core::RenderTextStyle;
    use muxr_core::SessionName;
    use muxr_core::TerminalSize;
    use test_that::prelude::*;

    use super::*;
    use crate::pane::focus::PaneFocusDirection;
    use crate::pane::split::PaneSplitAxis;
    use crate::state::SessionMetadata;

    #[test]
    fn test_attention_pane_ids_when_pane_needs_generic_attention_returns_pane() -> rootcause::Result<()> {
        let mut layout = self::layout()?;
        let pane_id = PaneId::new(1)?;
        let Some(pane) = layout.pane_mut(pane_id) else {
            return Err(report!("expected pane"));
        };
        pane.attention_state = PaneAttentionState::NeedsAttention;

        assert_that!(layout.attention_pane_ids(), eq(vec![pane_id]));
        Ok(())
    }

    #[test]
    fn test_focus_pane_direction_when_target_needs_generic_attention_clears_attention() -> rootcause::Result<()> {
        let mut layout = self::layout()?;
        let pane_id = PaneId::new(1)?;
        let Some(pane) = layout.pane_mut(pane_id) else {
            return Err(report!("expected pane"));
        };
        pane.attention_state = PaneAttentionState::NeedsAttention;

        assert_that!(
            layout.focus_pane_direction(&TerminalSize::new(80, 24)?, PaneFocusDirection::Left)?,
            eq(crate::pane::focus::PaneFocusChange::Changed)
        );

        assert_that!(layout.attention_pane_ids(), eq(Vec::<PaneId>::new()));
        Ok(())
    }

    #[test]
    fn test_apply_attention_tint_when_rgb_bg_is_present_blends_with_tint_and_preserves_fg_and_attrs() {
        let style = RenderStyle {
            attrs: RenderTextStyle::empty().set_italic(true),
            bg: RenderColor::Rgb { r: 20, g: 20, b: 20 },
            fg: RenderColor::Indexed(7),
        };

        let updated = apply_attention_tint(style, RenderColor::Rgb { r: 80, g: 0, b: 0 });

        assert_that!(updated.attrs.italic(), eq(true));
        assert_that!(updated.attrs.dim(), eq(false));
        assert_that!(updated.bg, not(eq(style.bg)));
        assert_that!(updated.fg, eq(style.fg));
    }

    #[test]
    fn test_apply_attention_tint_when_theme_relative_bg_is_present_uses_tint_and_keeps_default_fg() {
        let style = RenderStyle {
            attrs: RenderTextStyle::empty(),
            bg: RenderColor::Default,
            fg: RenderColor::Default,
        };
        let tint = RenderColor::Rgb { r: 80, g: 0, b: 0 };

        let updated = apply_attention_tint(style, tint);

        assert_that!(updated.bg, eq(tint));
        assert_that!(updated.fg, eq(style.fg));
    }

    fn layout() -> rootcause::Result<SessionLayout> {
        let session: SessionName = "work".parse()?;
        let mut layout = SessionLayout::initial(&session, self::metadata("sh", 1))?;
        layout.split_active_pane(
            MuxrConfig::default().layout,
            self::metadata("sh", 2),
            PaneSplitAxis::Vertical,
        )?;
        Ok(layout)
    }

    fn metadata(cmd_label: &str, started_at: u64) -> SessionMetadata {
        SessionMetadata {
            cmd_label: cmd_label.to_owned(),
            cwd: "/tmp".to_owned(),
            started_at,
        }
    }
}
