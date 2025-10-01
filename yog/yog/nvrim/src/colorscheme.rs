use color_eyre::eyre::eyre;
use nvim_oxi::Dictionary;
use nvim_oxi::api::opts::GetHighlightOpts;
use nvim_oxi::api::opts::GetHighlightOptsBuilder;
use nvim_oxi::api::opts::SetHighlightOpts;
use nvim_oxi::api::opts::SetHighlightOptsBuilder;
use nvim_oxi::api::types::GetHlInfos;
use nvim_oxi::api::types::HighlightInfos;

use crate::dict;
use crate::fn_from;

const BG: &str = "#002200";
const DIAGNOSTIC_LVLS: [&str; 5] = ["Error", "Warn", "Info", "Hint", "Ok"];
const STATUS_LINE_BG: &str = "none";

/// [`Dictionary`] with colorscheme and highlight helpers.
pub fn dict() -> Dictionary {
    dict! {
        "set": fn_from!(set),
    }
}

/// Sets the desired Neovim colorscheme and custom highlight groups.
#[allow(clippy::needless_pass_by_value)]
pub fn set(colorscheme: Option<String>) {
    if let Some(cs) = colorscheme {
        crate::oxi_ext::api::exec_vim_cmd("colorscheme", &[cs]);
    }

    let opts = crate::vim_opts::global_scope();
    crate::vim_opts::set("background", "dark", &opts);
    crate::vim_opts::set("termguicolors", true, &opts);

    let status_line_hl = set_opts().foreground("gray").background(STATUS_LINE_BG).build();
    let bg_hl = set_opts().background(BG).build();

    let general_hls = [
        ("ColorColumn", set_opts().background("NvimDarkGrey3").build()),
        ("Cursor", set_opts().foreground("black").background("white").build()),
        ("CursorLine", set_opts().foreground("none").build()),
        ("MsgArea", status_line_hl.clone()),
        ("Normal", bg_hl.clone()),
        ("NormalFloat", bg_hl),
        ("StatusLine", status_line_hl),
        ("TreesitterContext", set_opts().background("NvimDarkGrey3").build()),
    ];
    for (hl_name, hl_opts) in general_hls {
        set_hl(0, hl_name, &hl_opts);
    }

    let mut get_opts = get_opts();
    for lvl in DIAGNOSTIC_LVLS {
        let Ok(hl_infos) = get_hl(0, &get_opts.name(format!("Diagnostic{lvl}")).build()) else {
            continue;
        };
        let Ok(set_hl_opts) =
            hl_opts_from_hl_infos(&hl_infos).map(|mut hl_opts| hl_opts.background(STATUS_LINE_BG).build())
        else {
            continue;
        };
        set_hl(0, &format!("DiagnosticStatusLine{lvl}"), &set_hl_opts);

        let diagn_underline_hl = format!("DiagnosticUnderline{lvl}");
        let Ok(hl_infos) = get_hl(0, &get_opts.name(diagn_underline_hl.clone()).build()) else {
            continue;
        };
        let Ok(set_hl_opts) =
            hl_opts_from_hl_infos(&hl_infos).map(|mut hl_opts| hl_opts.background(STATUS_LINE_BG).build())
        else {
            continue;
        };
        set_hl(0, &diagn_underline_hl, &set_hl_opts);
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
        crate::oxi_ext::api::notify_error(&format!(
            "cannot set hl opts {hl_opts:#?} to {hl_name} on namespace {ns_id}, error {error:#?}"
        ));
    }
}

/// Retrieves [`HighlightInfos`] for a single group.
///
/// Errors:
/// - Propagates failures from [`nvim_oxi::api::get_hl`] while notifying them to Neovim.
/// - Returns an error in case of multiple infos ([`GetHlInfos::Map`]) for the given `hl_opts` .
///
/// # Errors
/// In case:
/// - Calling `nvim_get_hl` fails.
/// - Multiple highlight groups are returned instead of a single one.
fn get_hl(ns_id: u32, hl_opts: &GetHighlightOpts) -> color_eyre::Result<HighlightInfos> {
    nvim_oxi::api::get_hl(ns_id, hl_opts)
        .inspect_err(|error| {
            crate::oxi_ext::api::notify_error(&format!("cannot get HighlightInfos by {hl_opts:#?}, error {error:#?}"));
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

/// Builds a [`SetHighlightOptsBuilder`] from [`HighlightInfos`], applying only present fields via [`Option::map`].
///
/// Returns a [`color_eyre::Result`]. Errors if `blend` (`u32`) cannot convert to `u8` and notifies it to Neovim.
///
/// # Errors
/// In case:
/// - The `blend` value cannot fit into a `u8`.
fn hl_opts_from_hl_infos(hl_infos: &HighlightInfos) -> color_eyre::Result<SetHighlightOptsBuilder> {
    let mut opts = set_opts();
    hl_infos.altfont.map(|value| opts.altfont(value));
    hl_infos
        .background
        .map(|value| opts.background(&decimal_to_hex_color(value)));
    hl_infos.bg_indexed.map(|value| opts.bg_indexed(value));
    hl_infos
        .blend
        .map(u8::try_from)
        .transpose()
        .inspect_err(|error| {
            crate::oxi_ext::api::notify_error(&format!(
                "cannot convert u32 {:?} to u8, error: {error:#?}",
                hl_infos.blend
            ));
        })?
        .map(|value| opts.blend(value));
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
    Ok(opts)
}

/// Formats an RGB integer as a `#RRGGBB` hex string.
fn decimal_to_hex_color(decimal: u32) -> String {
    format!("#{decimal:06X}")
}
