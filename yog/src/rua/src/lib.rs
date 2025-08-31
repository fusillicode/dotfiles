//! Neovim Lua module with Rust utilities.

use nvim_oxi::Dictionary;

/// Generates CLI flags for fd and ripgrep.
mod cli_flags;
use crate::cli_flags::CliFlags;
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

/// The main plugin function that returns a [`Dictionary`] of Lua functions exposed to Neovim.
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
    }
}
