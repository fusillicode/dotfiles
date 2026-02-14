//! Colorscheme and highlight group configuration helpers.
//!
//! Exposes a dictionary with a `set` function applying base UI preferences (dark background, termguicolors)
//! and custom highlight groups (diagnostics, statusline, general UI).
//!
//! # Note on `set_hl` implementation
//!
//! Highlight options are set through a Lua-based path (`vim.api.nvim_set_hl`) instead of the
//! direct C FFI binding (`nvim_oxi::api::set_hl`). This works around an ABI mismatch between
//! `nvim-oxi`'s `SetHighlightOpts` struct layout (targeting Neovim v0.11.3) and Neovim
//! v0.12.0-dev (`886efcb853`), where the `cterm` field in `Dict(highlight)` changed from
//! `Object` (32 bytes) to `DictAs(highlight_cterm)` / `Dict` (24 bytes), shifting all
//! subsequent fields and causing validation errors.

use core::fmt;
use core::fmt::Display;

use nvim_oxi::Dictionary;
use nvim_oxi::api::SuperIterator;
use nvim_oxi::api::opts::GetHighlightOpts;
use nvim_oxi::api::opts::GetHighlightOptsBuilder;
use nvim_oxi::api::types::GetHlInfos;
use nvim_oxi::api::types::HighlightInfos;
use rootcause::report;

const GLOBAL_BG: &str = "#002020";
const GLOBAL_FG: &str = "#c9c9c9";

const CURSOR_BG: &str = "white";
const CURSOR_FG: &str = "black";
const NON_TEXT_FG: &str = "#777777";
const COMMENTS_FG: &str = "#777777";
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

    let non_text_hl = LuaHlOpts::new().fg(NON_TEXT_FG).bg(NONE);
    let statusline_hl = non_text_hl.clone().reverse(false);

    for (hl_name, hl_opts) in [
        ("Cursor", LuaHlOpts::new().fg(CURSOR_FG).bg(CURSOR_BG)),
        ("CursorLine", LuaHlOpts::new().fg(NONE)),
        ("ErrorMsg", LuaHlOpts::new().fg(DIAG_ERROR_FG)),
        ("MsgArea", LuaHlOpts::new().fg(COMMENTS_FG).bg(NONE)),
        ("LineNr", non_text_hl),
        ("Normal", LuaHlOpts::new().bg(GLOBAL_BG)),
        ("NormalFloat", LuaHlOpts::new().bg(GLOBAL_BG)),
        ("StatusLine", statusline_hl.clone()),
        ("StatusLineNC", statusline_hl),
        ("TreesitterContext", LuaHlOpts::new().bg(TREESITTER_CONTEXT_BG)),
        ("WinSeparator", LuaHlOpts::new().fg(TREESITTER_CONTEXT_BG)),
        // Changing these will change the main foreground color.
        ("@variable", LuaHlOpts::new().fg(GLOBAL_FG)),
        ("Comment", LuaHlOpts::new().fg(COMMENTS_FG)),
        ("Constant", LuaHlOpts::new().fg(GLOBAL_FG)),
        ("Delimiter", LuaHlOpts::new().fg(GLOBAL_FG)),
        // ("Function", LuaHlOpts::new().fg(FG)),
        ("PreProc", LuaHlOpts::new().fg(GLOBAL_FG)),
        ("Operator", LuaHlOpts::new().fg(GLOBAL_FG)),
        ("Statement", LuaHlOpts::new().fg(GLOBAL_FG).bold(true)),
        ("Type", LuaHlOpts::new().fg(GLOBAL_FG)),
    ] {
        set_hl(0, hl_name, &hl_opts);
    }

    for (lvl, fg) in DIAGNOSTICS_FG {
        // Errors are already notified by [`get_overridden_hl_opts`]
        let _ = get_overridden_hl_opts(
            &format!("Diagnostic{lvl}"),
            |hl_opts| hl_opts.fg(fg).bg(NONE).bold(true),
            None,
        )
        .map(|hl_opts| {
            set_hl(0, &format!("Diagnostic{lvl}"), &hl_opts);
            set_hl(0, &format!("DiagnosticStatusLine{lvl}"), &hl_opts);
        });

        let diag_underline_hl_name = format!("DiagnosticUnderline{lvl}");
        // Errors are already notified by [`get_overridden_hl_opts`]
        let _ = get_overridden_hl_opts(&diag_underline_hl_name, |hl_opts| hl_opts.special(fg).bg(NONE), None)
            .map(|hl_opts| set_hl(0, &diag_underline_hl_name, &hl_opts));
    }

    for (hl_name, fg) in GITSIGNS_FG {
        set_hl(0, hl_name, &LuaHlOpts::new().fg(fg));
    }
}

/// Retrieves the current highlight options for a given highlight group and applies overrides.
///
/// This function fetches the existing highlight information for the specified `hl_name`,
/// and then applies the provided `override_hl_opts` function to modify the options.
/// This is useful for incrementally changing highlight groups based on their current state.
///
/// # Errors
/// - If [`get_hl_single`] fails to retrieve the highlight info.
fn get_overridden_hl_opts(
    hl_name: &str,
    override_hl_opts: impl FnOnce(LuaHlOpts) -> LuaHlOpts,
    opts_builder: Option<GetHighlightOptsBuilder>,
) -> rootcause::Result<LuaHlOpts> {
    let mut get_hl_opts = opts_builder.unwrap_or_default();
    let hl_infos = get_hl_single(0, &get_hl_opts.name(hl_name).build())?;
    Ok(override_hl_opts(LuaHlOpts::from(&hl_infos)))
}

/// Sets a highlight group in the specified namespace via Lua, bypassing the broken C FFI path.
///
/// Executes `vim.api.nvim_set_hl(ns_id, hl_name, opts)` through Neovim's Lua interpreter.
/// On failure, it notifies the error to Neovim instead of propagating it, ensuring
/// the colorscheme setup continues gracefully.
fn set_hl(ns_id: u32, hl_name: &str, hl_opts: &LuaHlOpts) {
    let lua_cmd = format!("lua vim.api.nvim_set_hl({ns_id}, '{hl_name}', {hl_opts})",);

    if let Err(err) = nvim_oxi::api::command(&lua_cmd) {
        ytil_noxi::notify::error(format!(
            "error setting highlight opts | lua_cmd={lua_cmd:?} error={err:#?}"
        ));
    }
}

/// Retrieves [`HighlightInfos`] of a single group.
///
/// # Errors
/// - Propagates failures from [`nvim_oxi::api::get_hl`] while notifying them to Neovim.
/// - Returns an error in case of multiple infos ([`GetHlInfos::Map`]) for the given `hl_opts` .
fn get_hl_single(ns_id: u32, hl_opts: &GetHighlightOpts) -> rootcause::Result<HighlightInfos> {
    get_hl(ns_id, hl_opts).and_then(|hl| match hl {
        GetHlInfos::Single(highlight_infos) => Ok(highlight_infos),
        GetHlInfos::Map(hl_infos) => Err(report!(
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
) -> rootcause::Result<Vec<(nvim_oxi::String, HighlightInfos)>> {
    get_hl(ns_id, hl_opts).and_then(|hl| match hl {
        GetHlInfos::Single(hl_info) => Err(report!(
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
) -> rootcause::Result<GetHlInfos<impl SuperIterator<(nvim_oxi::String, HighlightInfos)>>> {
    nvim_oxi::api::get_hl(ns_id, hl_opts)
        .inspect_err(|err| {
            ytil_noxi::notify::error(format!(
                "cannot get highlight infos | hl_opts={hl_opts:#?} error={err:#?}"
            ));
        })
        .map_err(From::from)
}

/// Highlight options that serialize to a Lua table literal for use with
/// `vim.api.nvim_set_hl()`.
///
/// This bypasses the `nvim-oxi` C FFI struct (`SetHighlightOpts`) whose layout
/// diverges from Neovim master (see module-level docs).
#[derive(Clone, Debug, Default)]
struct LuaHlOpts {
    foreground: Option<String>,
    background: Option<String>,
    special_color: Option<String>,
    bold: Option<bool>,
    italic: Option<bool>,
    reverse: Option<bool>,
    standout: Option<bool>,
    strikethrough: Option<bool>,
    underline: Option<bool>,
    undercurl: Option<bool>,
    underdouble: Option<bool>,
    underdotted: Option<bool>,
    underdashed: Option<bool>,
    altfont: Option<bool>,
    nocombine: Option<bool>,
    fallback: Option<bool>,
    fg_indexed: Option<bool>,
    bg_indexed: Option<bool>,
    force: Option<bool>,
    blend: Option<u32>,
}

impl LuaHlOpts {
    fn new() -> Self {
        Self::default()
    }

    fn fg(mut self, color: &str) -> Self {
        self.foreground = Some(color.to_owned());
        self
    }

    fn bg(mut self, color: &str) -> Self {
        self.background = Some(color.to_owned());
        self
    }

    fn special(mut self, color: &str) -> Self {
        self.special_color = Some(color.to_owned());
        self
    }

    const fn bold(mut self, value: bool) -> Self {
        self.bold = Some(value);
        self
    }

    const fn reverse(mut self, value: bool) -> Self {
        self.reverse = Some(value);
        self
    }
}

impl From<&HighlightInfos> for LuaHlOpts {
    fn from(infos: &HighlightInfos) -> Self {
        let mut opts = Self::new();
        if let Some(v) = infos.foreground {
            opts.foreground = Some(decimal_to_hex_color(v));
        }
        if let Some(v) = infos.background {
            opts.background = Some(decimal_to_hex_color(v));
        }
        if let Some(v) = infos.special {
            opts.special_color = Some(decimal_to_hex_color(v));
        }
        opts.bold = infos.bold;
        opts.italic = infos.italic;
        opts.reverse = infos.reverse;
        opts.standout = infos.standout;
        opts.strikethrough = infos.strikethrough;
        opts.underline = infos.underline;
        opts.undercurl = infos.undercurl;
        opts.underdouble = infos.underlineline;
        opts.underdotted = infos.underdot;
        opts.underdashed = infos.underdash;
        opts.altfont = infos.altfont;
        opts.fallback = infos.fallback;
        opts.fg_indexed = infos.fg_indexed;
        opts.bg_indexed = infos.bg_indexed;
        opts.force = infos.force;
        opts.blend = infos.blend;
        opts
    }
}

impl Display for LuaHlOpts {
    /// Renders the options as a Lua table literal, e.g. `{ fg = 'black', bg = 'white', bold = true }`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut entries: Vec<String> = Vec::new();

        for (key, val) in [
            ("fg", &self.foreground),
            ("bg", &self.background),
            ("sp", &self.special_color),
        ] {
            if let Some(v) = val {
                entries.push(format!("{key} = '{v}'"));
            }
        }

        for (key, val) in [
            ("bold", self.bold),
            ("italic", self.italic),
            ("reverse", self.reverse),
            ("standout", self.standout),
            ("strikethrough", self.strikethrough),
            ("underline", self.underline),
            ("undercurl", self.undercurl),
            ("underdouble", self.underdouble),
            ("underdotted", self.underdotted),
            ("underdashed", self.underdashed),
            ("altfont", self.altfont),
            ("nocombine", self.nocombine),
            ("fallback", self.fallback),
            ("fg_indexed", self.fg_indexed),
            ("bg_indexed", self.bg_indexed),
            ("force", self.force),
        ] {
            if let Some(v) = val {
                entries.push(format!("{key} = {v}"));
            }
        }

        if let Some(v) = self.blend {
            entries.push(format!("blend = {v}"));
        }

        write!(f, "{{ {} }}", entries.join(", "))
    }
}

/// Formats an RGB integer as a `#RRGGBB` hex string.
fn decimal_to_hex_color(decimal: u32) -> String {
    format!("#{decimal:06X}")
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[test]
    fn from_default_highlight_infos_produces_default_lua_hl_opts() {
        let infos = HighlightInfos::default();
        let opts = LuaHlOpts::from(&infos);
        pretty_assertions::assert_eq!(opts.foreground, None);
        pretty_assertions::assert_eq!(opts.background, None);
        pretty_assertions::assert_eq!(opts.special_color, None);
        pretty_assertions::assert_eq!(opts.bold, None);
        pretty_assertions::assert_eq!(opts.blend, None);
    }

    #[rstest]
    #[case(0x00_00_00, "#000000")]
    #[case(0xFF_FF_FF, "#FFFFFF")]
    #[case(0xFF_00_00, "#FF0000")]
    #[case(0x00_20_20, "#002020")]
    fn from_highlight_infos_converts_foreground_to_hex(#[case] rgb: u32, #[case] expected: &str) {
        let mut infos = HighlightInfos::default();
        infos.foreground = Some(rgb);
        pretty_assertions::assert_eq!(LuaHlOpts::from(&infos).foreground.as_deref(), Some(expected));
    }

    #[rstest]
    #[case(0xFF_FF_FF, "#FFFFFF")]
    #[case(0x00_20_20, "#002020")]
    fn from_highlight_infos_converts_background_to_hex(#[case] rgb: u32, #[case] expected: &str) {
        let mut infos = HighlightInfos::default();
        infos.background = Some(rgb);
        pretty_assertions::assert_eq!(LuaHlOpts::from(&infos).background.as_deref(), Some(expected));
    }

    #[test]
    fn from_highlight_infos_converts_special_to_hex() {
        let mut infos = HighlightInfos::default();
        infos.special = Some(0xFF_00_00);
        pretty_assertions::assert_eq!(LuaHlOpts::from(&infos).special_color.as_deref(), Some("#FF0000"));
    }

    #[test]
    fn from_highlight_infos_maps_boolean_fields() {
        let mut infos = HighlightInfos::default();
        infos.bold = Some(true);
        infos.italic = Some(false);
        infos.underline = Some(true);
        infos.underdot = Some(true);
        infos.underdash = Some(true);
        infos.underlineline = Some(true);

        let opts = LuaHlOpts::from(&infos);
        pretty_assertions::assert_eq!(opts.bold, Some(true));
        pretty_assertions::assert_eq!(opts.italic, Some(false));
        pretty_assertions::assert_eq!(opts.underline, Some(true));
        pretty_assertions::assert_eq!(opts.underdotted, Some(true));
        pretty_assertions::assert_eq!(opts.underdashed, Some(true));
        pretty_assertions::assert_eq!(opts.underdouble, Some(true));
    }

    #[test]
    fn from_highlight_infos_maps_blend() {
        let mut infos = HighlightInfos::default();
        infos.blend = Some(50);
        pretty_assertions::assert_eq!(LuaHlOpts::from(&infos).blend, Some(50));
    }

    #[rstest]
    #[case(LuaHlOpts::new(), "{  }")]
    #[case(LuaHlOpts::new().fg("black"), "{ fg = 'black' }")]
    #[case(LuaHlOpts::new().bg("#002020"), "{ bg = '#002020' }")]
    #[case(LuaHlOpts::new().fg("#002020").bg("white"), "{ fg = '#002020', bg = 'white' }")]
    #[case(LuaHlOpts::new().bold(true), "{ bold = true }")]
    #[case(LuaHlOpts::new().special("red"), "{ sp = 'red' }")]
    #[case(LuaHlOpts { blend: Some(30), ..Default::default() }, "{ blend = 30 }")]
    #[case(
        LuaHlOpts::new().fg("black").bg("white").special("#FF0000").bold(true),
        "{ fg = 'black', bg = 'white', sp = '#FF0000', bold = true }",
    )]
    fn display_renders_lua_table(#[case] opts: LuaHlOpts, #[case] expected: &str) {
        pretty_assertions::assert_eq!(opts.to_string(), expected);
    }
}
