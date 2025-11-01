//! Colorscheme and highlight group configuration helpers.
//!
//! Exposes a dictionary with a `set` function applying base UI preferences (dark background, termguicolors)
//! and custom highlight groups (diagnostics, statusline, general UI).

use color_eyre::eyre::eyre;
use nvim_oxi::Dictionary;
use nvim_oxi::api::SuperIterator;
use nvim_oxi::api::opts::GetHighlightOpts;
use nvim_oxi::api::opts::GetHighlightOptsBuilder;
use nvim_oxi::api::opts::SetHighlightOpts;
use nvim_oxi::api::opts::SetHighlightOptsBuilder;
use nvim_oxi::api::types::GetHlInfos;
use nvim_oxi::api::types::HighlightInfos;

const BG: &str = "#002000";
const FG: &str = "#FFFFFF";
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
        let _ = ytil_nvim_oxi::api::exec_vim_cmd("colorscheme", Some(&[cs]));
    }

    let opts = crate::vim_opts::global_scope();
    crate::vim_opts::set("background", "dark", &opts);
    crate::vim_opts::set("termguicolors", true, &opts);

    let status_line_hl = get_default_hl_opts()
        .foreground("gray")
        .background(STATUS_LINE_BG)
        .build();
    let normal_hl = get_default_hl_opts().background(BG).build();

    let general_hls = [
        ("ColorColumn", get_default_hl_opts().background("NvimDarkGrey3").build()),
        (
            "Cursor",
            get_default_hl_opts().foreground("black").background("white").build(),
        ),
        ("CursorLine", get_default_hl_opts().foreground("none").build()),
        ("MsgArea", status_line_hl.clone()),
        ("Normal", normal_hl.clone()),
        ("NormalFloat", normal_hl),
        ("StatusLine", status_line_hl),
        (
            "TreesitterContext",
            get_default_hl_opts().background("NvimDarkGrey3").build(),
        ),
        // Changing these will change the main foreground color.
        ("@variable", get_default_hl_opts().foreground(FG).build()),
        ("Constant", get_default_hl_opts().foreground(FG).build()),
        ("Delimiter", get_default_hl_opts().foreground(FG).build()),
        // ("Function", get_default_hl_opts().foreground(FG).build()),
        ("Operator", get_default_hl_opts().foreground(FG).build()),
        ("Statement", get_default_hl_opts().foreground(FG).bold(true).build()),
        ("Type", get_default_hl_opts().foreground(FG).build()),
    ];
    for (hl_name, hl_opts) in general_hls {
        set_hl(0, hl_name, &hl_opts);
    }

    let mut get_hl_opts = GetHighlightOptsBuilder::default();
    for lvl in DIAGNOSTIC_LVLS {
        let Ok(hl_infos) = get_hl_single(0, &get_hl_opts.name(format!("Diagnostic{lvl}")).build()) else {
            continue;
        };
        let Ok(set_hl_opts) =
            hl_opts_from_hl_infos(&hl_infos).map(|mut hl_opts| hl_opts.background(STATUS_LINE_BG).build())
        else {
            continue;
        };
        set_hl(0, &format!("DiagnosticStatusLine{lvl}"), &set_hl_opts);

        let diagn_underline_hl = format!("DiagnosticUnderline{lvl}");
        let Ok(hl_infos) = get_hl_single(0, &get_hl_opts.name(diagn_underline_hl.clone()).build()) else {
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
fn get_default_hl_opts() -> SetHighlightOptsBuilder {
    SetHighlightOptsBuilder::default()
}

/// Wrapper around `nvim_oxi::api::set_hl` with error notification.
fn set_hl(ns_id: u32, hl_name: &str, hl_opts: &SetHighlightOpts) {
    if let Err(error) = nvim_oxi::api::set_hl(ns_id, hl_name, hl_opts) {
        ytil_nvim_oxi::api::notify_error(format!(
            "cannot set highlight opts | hl_opts={hl_opts:#?} hl_name={hl_name} namespace={ns_id} error={error:#?}"
        ));
    }
}

/// Retrieves [`HighlightInfos`] of a single group.
///
/// Errors:
/// - Propagates failures from [`nvim_oxi::api::get_hl`] while notifying them to Neovim.
/// - Returns an error in case of multiple infos ([`GetHlInfos::Map`]) for the given `hl_opts` .
///
/// # Errors
/// - Calling `nvim_get_hl` fails.
/// - Multiple highlight groups are returned instead of a single one.
fn get_hl_single(ns_id: u32, hl_opts: &GetHighlightOpts) -> color_eyre::Result<HighlightInfos> {
    get_hl(ns_id, hl_opts).and_then(|hl| match hl {
        GetHlInfos::Single(highlight_infos) => Ok(highlight_infos),
        GetHlInfos::Map(hl_infos) => Err(eyre!(
            "multiple highlight infos returned | hl_infos={:#?} hl_opts={hl_opts:#?}",
            hl_infos.collect::<Vec<_>>()
        )),
    })
}

/// Retrieves multiple [`HighlightInfos`] entries (map variant) for given highlight options.
///
/// Errors:
/// - Propagates failures from [`nvim_oxi::api::get_hl`] while notifying them to Neovim.
/// - Returns an error if only a single highlight group ([`GetHlInfos::Single`]) is returned.
///
/// # Errors
/// - Calling `nvim_get_hl` fails.
/// - A single highlight group is returned instead of multiple.
#[allow(dead_code)]
fn get_hl_multiple(
    ns_id: u32,
    hl_opts: &GetHighlightOpts,
) -> color_eyre::Result<Vec<(nvim_oxi::String, HighlightInfos)>> {
    get_hl(ns_id, hl_opts).and_then(|hl| match hl {
        GetHlInfos::Single(hl_info) => Err(eyre!(
            "single highlight info returned | hl_info={hl_info:#?} hl_opts={hl_opts:#?}",
        )),
        GetHlInfos::Map(hl_infos) => Ok(hl_infos.into_iter().collect()),
    })
}

/// Retrieves [`GetHlInfos`] (single or map) for given highlight options.
///
/// Errors:
/// - Propagates failures from [`nvim_oxi::api::get_hl`] while notifying them to Neovim.
///
/// # Errors
/// - Calling `nvim_get_hl` fails.
fn get_hl(
    ns_id: u32,
    hl_opts: &GetHighlightOpts,
) -> color_eyre::Result<GetHlInfos<impl SuperIterator<(nvim_oxi::String, HighlightInfos)>>> {
    nvim_oxi::api::get_hl(ns_id, hl_opts)
        .inspect_err(|error| {
            ytil_nvim_oxi::api::notify_error(format!(
                "cannot get highlight infos | hl_opts={hl_opts:#?} error={error:#?}"
            ));
        })
        .map_err(From::from)
}

/// Builds a [`SetHighlightOptsBuilder`] from [`HighlightInfos`], applying only present fields via [`Option::map`].
///
/// Returns a [`color_eyre::Result`]. Errors if `blend` (`u32`) cannot convert to `u8` and notifies it to Neovim.
///
/// # Errors
/// - The `blend` value cannot fit into a `u8`.
fn hl_opts_from_hl_infos(hl_infos: &HighlightInfos) -> color_eyre::Result<SetHighlightOptsBuilder> {
    let mut opts = get_default_hl_opts();
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
            ytil_nvim_oxi::api::notify_error(format!(
                "cannot convert blend value to u8 | value={:?} error={error:#?}",
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
