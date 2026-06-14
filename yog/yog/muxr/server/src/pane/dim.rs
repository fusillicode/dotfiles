use muxr_config::PaneDimConfig;
use muxr_core::RenderColor;
use muxr_core::RenderStyle;

pub fn apply_dim_style(mut style: RenderStyle, pane_dim: PaneDimConfig) -> RenderStyle {
    // SGR dim is terminal/theme-dependent for explicit prompt colors; darken concrete colors here instead of stacking
    // terminal dim on top, so inactive prompt segments stay muted without becoming unreadable.
    let has_explicit_color = !matches!(style.bg, RenderColor::Default) || !matches!(style.fg, RenderColor::Default);
    if !has_explicit_color {
        style.attrs = style.attrs.set_dim(true);
    }
    style.bg = dim_explicit_color(style.bg, pane_dim);
    style.fg = dim_explicit_color(style.fg, pane_dim);
    style
}

fn dim_explicit_color(color: RenderColor, pane_dim: PaneDimConfig) -> RenderColor {
    match color {
        RenderColor::Default => RenderColor::Default,
        RenderColor::Indexed(index) => {
            let (r, g, b) = xterm_indexed_rgb(index);
            RenderColor::Rgb {
                r: dim_channel(r, pane_dim.explicit_color_percent),
                g: dim_channel(g, pane_dim.explicit_color_percent),
                b: dim_channel(b, pane_dim.explicit_color_percent),
            }
        }
        RenderColor::Rgb { r, g, b } => RenderColor::Rgb {
            r: dim_channel(r, pane_dim.explicit_color_percent),
            g: dim_channel(g, pane_dim.explicit_color_percent),
            b: dim_channel(b, pane_dim.explicit_color_percent),
        },
    }
}

fn xterm_indexed_rgb(index: u8) -> (u8, u8, u8) {
    const ANSI_COLORS: [(u8, u8, u8); 16] = [
        (0, 0, 0),
        (128, 0, 0),
        (0, 128, 0),
        (128, 128, 0),
        (0, 0, 128),
        (128, 0, 128),
        (0, 128, 128),
        (192, 192, 192),
        (128, 128, 128),
        (255, 0, 0),
        (0, 255, 0),
        (255, 255, 0),
        (0, 0, 255),
        (255, 0, 255),
        (0, 255, 255),
        (255, 255, 255),
    ];
    const CUBE_LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];

    if let Some(color) = ANSI_COLORS.get(usize::from(index)) {
        return *color;
    }
    if index < 232 {
        let cube = index.saturating_sub(16);
        let red = cube.checked_div(36).unwrap_or_default();
        let green = cube
            .checked_rem(36)
            .unwrap_or_default()
            .checked_div(6)
            .unwrap_or_default();
        let blue = cube.checked_rem(6).unwrap_or_default();
        return (
            *CUBE_LEVELS.get(usize::from(red)).unwrap_or(&0),
            *CUBE_LEVELS.get(usize::from(green)).unwrap_or(&0),
            *CUBE_LEVELS.get(usize::from(blue)).unwrap_or(&0),
        );
    }

    let gray = 8_u8.saturating_add(index.saturating_sub(232).saturating_mul(10));
    (gray, gray, gray)
}

fn dim_channel(channel: u8, percent: u8) -> u8 {
    const PERCENT_DENOMINATOR: u16 = 100;
    let percent = percent.min(100);
    let value = u16::from(channel)
        .saturating_mul(u16::from(percent))
        .checked_div(PERCENT_DENOMINATOR)
        .unwrap_or_default();
    u8::try_from(value).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use muxr_core::RenderTextStyle;

    use super::*;

    #[test]
    fn test_apply_dim_style_when_default_colors_uses_terminal_dim_attr() {
        let style = RenderStyle {
            attrs: RenderTextStyle::empty().set_bold(true),
            bg: RenderColor::Default,
            fg: RenderColor::Default,
        };

        let updated = apply_dim_style(
            style,
            PaneDimConfig {
                explicit_color_percent: 80,
                unfocused: true,
            },
        );

        assert2::assert!(updated.attrs.bold());
        assert2::assert!(updated.attrs.dim());
        pretty_assertions::assert_eq!(updated.bg, style.bg);
        pretty_assertions::assert_eq!(updated.fg, style.fg);
    }

    #[test]
    fn test_apply_dim_style_when_explicit_fg_darkens_without_terminal_dim_attr() {
        let style = RenderStyle {
            attrs: RenderTextStyle::empty().set_bold(true),
            bg: RenderColor::Default,
            fg: RenderColor::Indexed(7),
        };

        let updated = apply_dim_style(
            style,
            PaneDimConfig {
                explicit_color_percent: 80,
                unfocused: true,
            },
        );

        assert2::assert!(updated.attrs.bold());
        assert2::assert!(!updated.attrs.dim());
        pretty_assertions::assert_eq!(updated.bg, style.bg);
        assert2::assert!(updated.fg != style.fg);
    }
}
