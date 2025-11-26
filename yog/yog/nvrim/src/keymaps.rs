//! Keymap helpers and expr RHS generators.
//!
//! Provides `keymaps.dict()` offering bulk keymap setup (`set_all`) plus smart editing helpers.
//! Core mappings target Normal / Visual / Operator modes; failures in individual definitions are
//! logged without aborting subsequent mappings.

use ytil_nvim_oxi::Dictionary;
use ytil_nvim_oxi::api::opts::SetKeymapOpts;
use ytil_nvim_oxi::api::opts::SetKeymapOptsBuilder;
use ytil_nvim_oxi::api::types::Mode;

const NV_MODE: [Mode; 2] = [Mode::Normal, Mode::Visual];
const NVOP_MODE: [Mode; 1] = [Mode::NormalVisualOperator];

/// RHS used for the normal-mode `<Esc>` mapping (clear search + empty echo).
pub const NORMAL_ESC: &str = r#":noh<cr>:echo""<cr>"#;

/// [`Dictionary`] of keymap helpers and expr RHS generators.
pub fn dict() -> Dictionary {
    dict! {
        "set_all": fn_from!(set_all),
        "smart_ident_on_blank_line": fn_from!(smart_ident_on_blank_line),
        "smart_dd_no_yank_empty_line": fn_from!(smart_dd_no_yank_empty_line),
        "normal_esc": NORMAL_ESC,
        "visual_esc": fn_from!(visual_esc),
    }
}

/// Set a keymap for each provided [`Mode`].
///
/// Errors are reported (not propagated) via `ytil_nvim_oxi::notify::error`.
pub fn set(modes: &[Mode], lhs: &str, rhs: &str, opts: &SetKeymapOpts) {
    for mode in modes {
        if let Err(err) = nvim_oxi::api::set_keymap(*mode, lhs, rhs, opts) {
            ytil_nvim_oxi::notify::error(format!(
                "cannot set keymap | mode={mode:#?} lhs={lhs} rhs={rhs} opts={opts:#?} error={err:#?}"
            ));
        }
    }
}

/// Build the default [`SetKeymapOpts`]: silent + non-recursive.
///
/// Unlike Lua's `vim.keymap.set`, the default for [`SetKeymapOpts`] does *not*
/// enable `noremap` so we do it explicitly.
pub fn default_opts() -> SetKeymapOpts {
    SetKeymapOptsBuilder::default().silent(true).noremap(true).build()
}

/// Sets the core (non‑plugin) keymaps ported from the Lua `M.setup` function.
///
/// All mappings are set with the default non-recursive, silent options returned
/// by [`default_opts`].
///
/// Failures are reported internally by the [`set`] helper via `ytil_nvim_oxi::notify::error`.
fn set_all(_: ()) {
    let default_opts = default_opts();

    ytil_nvim_oxi::common::set_g_var("mapleader", " ");
    ytil_nvim_oxi::common::set_g_var("maplocalleader", " ");

    set(&[Mode::Terminal], "<Esc>", "<c-\\><c-n>", &default_opts);
    set(&[Mode::Insert], "<c-a>", "<esc>^i", &default_opts);
    set(&[Mode::Normal], "<c-a>", "^i", &default_opts);
    set(&[Mode::Insert], "<c-e>", "<end>", &default_opts);
    set(&[Mode::Normal], "<c-e>", "$a", &default_opts);

    set(&NVOP_MODE, "gn", ":bn<cr>", &default_opts);
    set(&NVOP_MODE, "gp", ":bp<cr>", &default_opts);
    set(&NV_MODE, "gh", "0", &default_opts);
    set(&NV_MODE, "gl", "$", &default_opts);
    set(&NV_MODE, "gs", "_", &default_opts);

    set(&NV_MODE, "x", r#""_x"#, &default_opts);
    set(&NV_MODE, "X", r#""_X"#, &default_opts);

    set(
        &NV_MODE,
        "<leader>yf",
        r#":let @+ = expand("%") . ":" . line(".")<cr>"#,
        &default_opts,
    );
    set(&[Mode::Visual], "y", "ygv<esc>", &default_opts);
    set(&[Mode::Visual], "p", r#""_dP"#, &default_opts);

    set(&[Mode::Visual], ">", ">gv", &default_opts);
    set(&[Mode::Visual], "<", "<gv", &default_opts);
    set(&[Mode::Normal], ">", ">>", &default_opts);
    set(&[Mode::Normal], "<", "<<", &default_opts);
    set(&NV_MODE, "U", "<c-r>", &default_opts);

    set(&NV_MODE, "<leader><leader>", ":silent :w!<cr>", &default_opts);
    set(&NV_MODE, "<leader>x", ":bd<cr>", &default_opts);
    set(&NV_MODE, "<leader>X", ":bd!<cr>", &default_opts);
    set(&NV_MODE, "<leader>q", ":q<cr>", &default_opts);
    set(&NV_MODE, "<leader>Q", ":q!<cr>", &default_opts);

    set(&NV_MODE, "<c-;>", ":set wrap!<cr>", &default_opts);
    set(&[Mode::Normal], "<esc>", r#":noh<cr>:echo""<cr>"#, &default_opts);
}

/// Return the RHS for a smart normal-mode `i` mapping.
///
/// If the current line is blank, returns `"_cc` (replace line without yanking);
/// otherwise returns `i`.
///
/// Intended to be used with an *expr* mapping and `.expr(true)` in [`SetKeymapOpts`].
fn smart_ident_on_blank_line(_: ()) -> String {
    apply_on_current_line_or_unwrap(|line| if line.is_empty() { r#""_cc"# } else { "i" }, "i")
}

/// Return the RHS for a smart `dd` mapping that skips yanking blank lines.
///
/// Produces `"_dd` when the current line is entirely whitespace; otherwise `dd`.
///
/// Intended for an *expr* mapping.
fn smart_dd_no_yank_empty_line(_: ()) -> String {
    apply_on_current_line_or_unwrap(
        |line| {
            if line.chars().all(char::is_whitespace) {
                r#""_dd"#
            } else {
                "dd"
            }
        },
        "dd",
    )
}

/// Return the RHS for a visual-mode `<Esc>` *expr* mapping that reselects the
/// visual range (direction aware) and then applies [`NORMAL_ESC`].
fn visual_esc(_: ()) -> String {
    let current_line: i64 = nvim_oxi::api::call_function("line", (".",))
        .inspect_err(|err| {
            ytil_nvim_oxi::notify::error(format!("error getting current line | error={err:#?}"));
        })
        .unwrap_or(0);
    let visual_line: i64 = nvim_oxi::api::call_function("line", ("v",))
        .inspect_err(|err| {
            ytil_nvim_oxi::notify::error(format!("error getting visual line | error={err:#?}"));
        })
        .unwrap_or(0);
    format!(
        ":<c-u>'{}<cr>{}",
        if current_line < visual_line { "<" } else { ">" },
        NORMAL_ESC
    )
}

/// Apply a closure to the current line or fall back to `default`.
///
/// Used by the smart *expr* mapping helpers.
///
/// Errors from [`nvim_oxi::api::get_current_line`] are reported and the `default` is returned.
fn apply_on_current_line_or_unwrap<'a, F: FnOnce(String) -> &'a str>(fun: F, default: &'a str) -> String {
    nvim_oxi::api::get_current_line()
        .inspect_err(|err| {
            ytil_nvim_oxi::notify::error(format!("error getting current line | error={err:#?}"));
        })
        .map(fun)
        .unwrap_or(default)
        .to_string()
}
