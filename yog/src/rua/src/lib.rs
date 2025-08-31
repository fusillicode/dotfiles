//! Neovim Lua module exposing Rust utilities.

use nvim_oxi::Dictionary;

/// Generates CLI flags for fd and ripgrep.
mod cli_flags;
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
/// Utilities for working with visual selections.
mod visual_selection;

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
        "get_visual_selection": fn_from!(visual_selection::get),
    }
}
