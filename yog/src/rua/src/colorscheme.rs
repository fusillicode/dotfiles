use color_eyre::eyre::eyre;
use nvim_oxi::api::opts::GetHighlightOpts;
use nvim_oxi::api::opts::GetHighlightOptsBuilder;
use nvim_oxi::api::opts::SetHighlightOpts;
use nvim_oxi::api::opts::SetHighlightOptsBuilder;
use nvim_oxi::api::types::GetHlInfos;
use nvim_oxi::api::types::HighlightInfos;

const BG: &str = "#001900";
const DIAGNOSTIC_LVLS: [&str; 5] = ["Error", "Warn", "Info", "Hint", "Ok"];
const STATUS_LINE_HL_BG: &str = "none";

/// Sets the desired Neovim highlight groups.
pub fn set(_: ()) {
    let status_line_hl = set_opts().foreground("gray").background(STATUS_LINE_HL_BG).build();
    let bg_hl = set_opts().background(BG).build();

    let general_hls = [
        ("ColorColumn", set_opts().background("NvimDarkGrey3").build()),
        ("Cursor", set_opts().foreground("black").background("white").build()),
        ("CursorLine", set_opts().foreground("none").build()),
        ("MsgArea", status_line_hl.clone()),
        ("Normal", bg_hl.clone()),
        ("NormalFloat", bg_hl),
        ("StatusLine", status_line_hl),
    ];
    for (hl_name, hl_opts) in general_hls {
        set_hl(0, hl_name, &hl_opts);
    }

    let mut get_opts = get_opts();
    for lvl in DIAGNOSTIC_LVLS {
        let diagn_hl = format!("Diagnostic{lvl}");
        let Ok(hl_infos) = get_hl(0, &get_opts.name(diagn_hl.clone()).build()) else {
            continue;
        };
        let hl_opts = hl_opts_from_hl_infos(hl_infos).background(STATUS_LINE_HL_BG).build();
        set_hl(0, &format!("DiagnosticStatusLine{lvl}{diagn_hl}"), &hl_opts);

        let diagn_underline_hl = format!("DiagnosticUnderline{lvl}");
        let Ok(hl_infos) = get_hl(0, &get_opts.name(diagn_underline_hl.clone()).build()) else {
            continue;
        };
        let hl_opts = hl_opts_from_hl_infos(hl_infos).undercurl(true).build();
        set_hl(0, &diagn_underline_hl, &hl_opts);
    }
}

/// Shorthand to start building [`SetHighlightOpts`].
fn set_opts() -> SetHighlightOptsBuilder {
    SetHighlightOptsBuilder::default()
}

/// Shorthand to start building [`GetHighlightOpts`].
fn get_opts() -> GetHighlightOptsBuilder {
    GetHighlightOptsBuilder::default()
}

/// Wrapper around `nvim_oxi::api::set_hl` with error notification.
fn set_hl(ns_id: u32, hl_name: &str, hl_opts: &SetHighlightOpts) {
    if let Err(error) = nvim_oxi::api::set_hl(ns_id, hl_name, hl_opts) {
        crate::oxi_ext::notify_error(&format!("foo, error {error:#?}"));
    }
}

/// Retrieves [`HighlightInfos`] for a single group or errors if multiple.
fn get_hl(ns_id: u32, hl_opts: &GetHighlightOpts) -> color_eyre::Result<HighlightInfos> {
    nvim_oxi::api::get_hl(ns_id, hl_opts)
        .inspect_err(|error| {
            crate::oxi_ext::notify_error(&format!("cannot get HighlightInfos by {hl_opts:#?}, error {error:#?}"))
        })
        .map_err(From::from)
        .and_then(|hl| match hl {
            GetHlInfos::Single(highlight_infos) => Ok(highlight_infos),
            GetHlInfos::Map(x) => Err(eyre!(
                "unexpected multiple HighlightInfos {:#?} for {hl_opts:#?}",
                x.collect::<Vec<_>>()
            )),
        })
}

/// Converts [`HighlightInfos`] into a [`SetHighlightOptsBuilder`].
/// Only applies fields present in the source using [`Option::map`].
fn hl_opts_from_hl_infos(hl_infos: HighlightInfos) -> SetHighlightOptsBuilder {
    let mut opts = set_opts();
    hl_infos.altfont.map(|value| opts.altfont(value));
    hl_infos
        .background
        .map(|value| opts.background(&decimal_to_hex_color(value)));
    hl_infos.bg_indexed.map(|value| opts.bg_indexed(value));
    hl_infos.blend.map(|value| opts.blend(value as _));
    hl_infos.bold.map(|value| opts.bold(value));
    hl_infos.fallback.map(|value| opts.fallback(value));
    hl_infos.fg_indexed.map(|value| opts.fg_indexed(value));
    hl_infos.force.map(|value| opts.force(value));
    hl_infos
        .foreground
        .map(|value| opts.foreground(&decimal_to_hex_color(value)));
    hl_infos.italic.map(|value| opts.italic(value));
    hl_infos.reverse.map(|value| opts.reverse(value));
    hl_infos.special.map(|value| opts.special(&decimal_to_hex_color(value)));
    hl_infos.standout.map(|value| opts.standout(value));
    hl_infos.strikethrough.map(|value| opts.strikethrough(value));
    hl_infos.undercurl.map(|value| opts.undercurl(value));
    hl_infos.underdash.map(|value| opts.underdashed(value));
    hl_infos.underdot.map(|value| opts.underdotted(value));
    hl_infos.underline.map(|value| opts.underline(value));
    opts
}

/// Formats an RGB integer as a `#RRGGBB` hex string.
fn decimal_to_hex_color(decimal: u32) -> String {
    format!("#{:06X}", decimal)
}
