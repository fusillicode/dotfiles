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

const GLOBAL_BG: &str = "#002020";
const GLOBAL_FG: &str = "#dcdcd7";

const CURSOR_BG: &str = "white";
const CURSOR_FG: &str = "black";
const NON_TEXT_FG: &str = "NvimDarkGrey4";
const COMMENTS_FG: &str = "NvimLightGrey4";
const NONE: &str = "none";

const DIAG_ERROR_FG: &str = "#ec635c";
const DIAG_OK_FG: &str = "#8ce479";
const DIAG_WARN_FG: &str = "#ffaa33";
const DIAG_HINT_FG: &str = "NvimLightGrey3";
const DIAG_INFO_FG: &str = "white";

const GITSIGNS_ADDED: &str = DIAG_OK_FG;
const GITSIGNS_CHANGED: &str = "#6a6adf";
const GITSIGNS_REMOVED: &str = DIAG_ERROR_FG;

const TREESITTER_CONTEXT_BG: &str = "NvimDarkGrey3";

const DIAGNOSTICS_FG: [(&str, &str); 5] = [
    ("Error", DIAG_ERROR_FG),
    ("Warn", DIAG_WARN_FG),
    ("Ok", DIAG_OK_FG),
    ("Hint", DIAG_HINT_FG),
    ("Info", DIAG_INFO_FG),
];

const GITSIGNS_FG: [(&str, &str); 3] = [
    ("Added", GITSIGNS_ADDED),
    ("Changed", GITSIGNS_CHANGED),
    ("Removed", GITSIGNS_REMOVED),
];

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
        let _ = ytil_noxi::common::exec_vim_cmd("colorscheme", Some(&[cs]));
    }

    let opts = crate::vim_opts::global_scope();
    crate::vim_opts::set("background", "dark", &opts);
    crate::vim_opts::set("termguicolors", true, &opts);

    let non_text_hl = get_default_hl_opts().foreground(NON_TEXT_FG).background(NONE).build();

    for (hl_name, hl_opts) in [
        (
            "Cursor",
            get_default_hl_opts()
                .foreground(CURSOR_FG)
                .background(CURSOR_BG)
                .build(),
        ),
        ("CursorLine", get_default_hl_opts().foreground(NONE).build()),
        ("ErrorMsg", get_default_hl_opts().foreground(DIAG_ERROR_FG).build()),
        (
            "MsgArea",
            get_default_hl_opts().foreground(COMMENTS_FG).background(NONE).build(),
        ),
        ("LineNr", non_text_hl.clone()),
        ("Normal", get_default_hl_opts().background(GLOBAL_BG).build()),
        ("NormalFloat", get_default_hl_opts().background(GLOBAL_BG).build()),
        ("StatusLine", non_text_hl),
        (
            "TreesitterContext",
            get_default_hl_opts().background(TREESITTER_CONTEXT_BG).build(),
        ),
        (
            "WinSeparator",
            get_default_hl_opts().foreground(TREESITTER_CONTEXT_BG).build(),
        ),
        // Changing these will change the main foreground color.
        ("@variable", get_default_hl_opts().foreground(GLOBAL_FG).build()),
        ("Comment", get_default_hl_opts().foreground(COMMENTS_FG).build()),
        ("Constant", get_default_hl_opts().foreground(GLOBAL_FG).build()),
        ("Delimiter", get_default_hl_opts().foreground(GLOBAL_FG).build()),
        // ("Function", get_default_hl_opts().foreground(FG).build()),
        ("PreProc", get_default_hl_opts().foreground(GLOBAL_FG).build()),
        ("Operator", get_default_hl_opts().foreground(GLOBAL_FG).build()),
        (
            "Statement",
            get_default_hl_opts().foreground(GLOBAL_FG).bold(true).build(),
        ),
        ("Type", get_default_hl_opts().foreground(GLOBAL_FG).build()),
    ] {
        set_hl(0, hl_name, &hl_opts);
    }

    for (lvl, fg) in DIAGNOSTICS_FG {
        // Errors are already notified by [`get_overridden_set_hl_opts`]
        let _ = get_overridden_set_hl_opts(
            &format!("Diagnostic{lvl}"),
            |mut hl_opts| hl_opts.foreground(fg).background(NONE).bold(true).build(),
            None,
        )
        .map(|set_hl_opts| {
            set_hl(0, &format!("Diagnostic{lvl}"), &set_hl_opts);
            set_hl(0, &format!("DiagnosticStatusLine{lvl}"), &set_hl_opts);
        });

        let diag_underline_hl_name = format!("DiagnosticUnderline{lvl}");
        // Errors are already notified by [`get_overridden_set_hl_opts`]
        let _ = get_overridden_set_hl_opts(
            &diag_underline_hl_name,
            |mut hl_opts| hl_opts.special(fg).background(NONE).build(),
            None,
        )
        .map(|set_hl_opts| set_hl(0, &diag_underline_hl_name, &set_hl_opts));
    }

    for (hl_name, fg) in GITSIGNS_FG {
        set_hl(0, hl_name, &get_default_hl_opts().foreground(fg).build());
    }
}

/// Retrieves the current highlight options for a given highlight group and applies overrides.
///
/// This function fetches the existing highlight information for the specified `hl_name`,
/// and then applies the provided `override_set_hl_opts` function to modify the options.
/// This is useful for incrementally changing highlight groups based on their current state.
///
/// # Arguments
/// - `hl_name` The name of the highlight group to retrieve and modify.
/// - `override_set_hl_opts` A closure that takes a [`SetHighlightOptsBuilder`] (pre-filled with the current highlight
///   options) and returns a modified [`SetHighlightOpts`].
/// - `opts_builder` Optional builder for customizing how the highlight info is retrieved. Defaults to
///   [`GetHighlightOptsBuilder::default()`] if [`None`].
///
/// # Errors
/// - If [`get_hl_single`] fails to retrieve the highlight info.
/// - If [`hl_opts_from_hl_infos`] fails to convert the highlight info.
fn get_overridden_set_hl_opts(
    hl_name: &str,
    override_set_hl_opts: impl FnMut(SetHighlightOptsBuilder) -> SetHighlightOpts,
    opts_builder: Option<GetHighlightOptsBuilder>,
) -> color_eyre::Result<SetHighlightOpts> {
    let mut get_hl_opts = opts_builder.unwrap_or_default();
    let hl_infos = get_hl_single(0, &get_hl_opts.name(hl_name).build())?;
    hl_opts_from_hl_infos(&hl_infos).map(override_set_hl_opts)
}

/// Shorthand to start building [`SetHighlightOpts`].
fn get_default_hl_opts() -> SetHighlightOptsBuilder {
    SetHighlightOptsBuilder::default()
}

/// Sets a highlight group in the specified namespace, with error handling via Neovim notifications.
///
/// This function wraps [`nvim_oxi::api::set_hl`] to apply highlight options to a group.
/// On failure, it notifies the error to Neovim instead of propagating it, ensuring
/// the colorscheme setup continues gracefully.
///
/// # Arguments
/// - `ns_id`: The namespace ID (0 for global).
/// - `hl_name`: The name of the highlight group to set.
/// - `hl_opts`: The highlight options to apply.
///
/// # Errors
/// Errors are notified to Neovim but not returned; the function always succeeds externally.
fn set_hl(ns_id: u32, hl_name: &str, hl_opts: &SetHighlightOpts) {
    if let Err(err) = nvim_oxi::api::set_hl(ns_id, hl_name, hl_opts) {
        ytil_noxi::notify::error(format!(
            "error setting highlight opts | hl_opts={hl_opts:#?} hl_name={hl_name:?} namespace={ns_id:?} error={err:#?}"
        ));
    }
}

/// Retrieves [`HighlightInfos`] of a single group.
///
/// # Errors
/// - Propagates failures from [`nvim_oxi::api::get_hl`] while notifying them to Neovim.
/// - Returns an error in case of multiple infos ([`GetHlInfos::Map`]) for the given `hl_opts` .
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
/// # Errors
/// - Propagates failures from [`nvim_oxi::api::get_hl`] while notifying them to Neovim.
fn get_hl(
    ns_id: u32,
    hl_opts: &GetHighlightOpts,
) -> color_eyre::Result<GetHlInfos<impl SuperIterator<(nvim_oxi::String, HighlightInfos)>>> {
    nvim_oxi::api::get_hl(ns_id, hl_opts)
        .inspect_err(|err| {
            ytil_noxi::notify::error(format!(
                "cannot get highlight infos | hl_opts={hl_opts:#?} error={err:#?}"
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
        .inspect_err(|err| {
            ytil_noxi::notify::error(format!(
                "cannot convert blend value to u8 | value={:?} error={err:#?}",
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
