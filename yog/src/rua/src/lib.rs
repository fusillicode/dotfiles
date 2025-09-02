//! Neovim Lua module exposing Rust-powered helpers for my Neovim config.
//!
//! Exposes functions callable from Lua via nvim-oxi. High-level areas:
//! - Diagnostics: filter/format/sort and render in statusline/statuscolumn
//! - CLI flags: generate ripgrep/fd arguments with sane defaults and blacklist
//! - Visual selections: get current buffer text for a visual range
//! - Test runner: run Rust tests in a sibling Wezterm pane
//! - Misc: fkr fake data commands and oxi extensions

use nvim_oxi::Dictionary;

/// Utilities for working with [`nvim_oxi::Buffer`] text.
mod buffer_text;
/// Generates CLI flags for fd and ripgrep.
mod cli_flags;
/// Sets the coloroscheme.
mod colorscheme;
/// Processes diagnostics for filtering, formatting, and sorting.
mod diagnostics;
/// Creates Neovim commands to generate fake data via [`fkr`] lib.
mod fkr;
/// Extends [`nvim_oxi`] with various utilities.
mod oxi_ext;
/// Draws status column with diagnostic and git signs.
mod statuscolumn;
/// Draws status line with diagnostic information.
mod statusline;
/// Runs tests at cursor position in an available Wezterm pane.
mod test_runner;

use crate::cli_flags::CliFlags;

/// The main plugin function that returns a [`Dictionary`] of [`Function`]s exposed to Neovim.
#[nvim_oxi::plugin]
fn rua() -> Dictionary {
    dict! {
        "format_diagnostic": fn_from!(diagnostics::formatter::format),
        "sort_diagnostics": fn_from!(diagnostics::sorter::sort),
        "filter_diagnostics": fn_from!(diagnostics::filter::filter),
        "draw_statusline": fn_from!(statusline::draw),
        "draw_statuscolumn": fn_from!(statuscolumn::draw),
        "create_fkr_cmds": fn_from!(fkr::create_cmds),
        "get_fd_cli_flags": fn_from!(cli_flags::fd::FdCliFlags::get),
        "get_rg_cli_flags": fn_from!(cli_flags::rg::RgCliFlags::get),
        "run_test": fn_from!(test_runner::run_test),
        "get_current_buffer_text": fn_from!(buffer_text::between_pos::get),
        "get_word_under_cursor": fn_from!(buffer_text::word_under_cursor::get),
        "set_colorscheme": fn_from!(colorscheme::set),
    }
}
