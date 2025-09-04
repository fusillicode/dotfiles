use nvim_oxi::Dictionary;
use nvim_oxi::api::opts::SetKeymapOpts;
use nvim_oxi::api::opts::SetKeymapOptsBuilder;
use nvim_oxi::api::types::Mode;

use crate::dict;
use crate::fn_from;

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
/// Errors are reported (not propagated) via [`crate::oxi_ext::notify_error`].
pub fn set(modes: &[Mode], lhs: &str, rhs: &str, opts: &SetKeymapOpts) {
    for mode in modes {
        if let Err(error) = nvim_oxi::api::set_keymap(*mode, lhs, rhs, opts) {
            crate::oxi_ext::notify_error(&format!(
                "cannot set keymap with mode {mode:#?}, lhs {lhs}, rhs {rhs} and opts {opts:#?}, error {error:#?}"
            ));
        }
    }
}

/// Sets the core (non‑plugin) keymaps ported from the Lua `M.setup` function.
///
/// All mappings are set with the default non-recursive, silent options returned
/// by [`default_opts`].
///
/// Failures are reported internally by the [`set`] helper via [`crate::oxi_ext::notify_error`].
fn set_all(_: ()) {
    let default_opts = default_opts();

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
        .inspect_err(|error| {
            crate::oxi_ext::notify_error(&format!("cannot get current line, error {error:#?}"));
        })
        .unwrap_or(0);
    let visual_line: i64 = nvim_oxi::api::call_function("line", ("v",))
        .inspect_err(|error| {
            crate::oxi_ext::notify_error(&format!("cannot get visual line, error {error:#?}"));
        })
        .unwrap_or(0);
    format!(
        ":<c-u>'{}{}",
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
        .inspect_err(|error| {
            crate::oxi_ext::notify_error(&format!("cannot get current line, error {error:#?}"));
        })
        .map(fun)
        .unwrap_or(default)
        .to_string()
}

/// Build the default [`SetKeymapOpts`]: silent + non-recursive.
///
/// Unlike Lua's [`vim.keymap.set`], the default for [`SetKeymapOpts`] does *not*
/// enable `noremap` so we do it explicitly.
fn default_opts() -> SetKeymapOpts {
    SetKeymapOptsBuilder::default().silent(true).noremap(true).build()
}
