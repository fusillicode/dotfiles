//! Neovim Lua module with Rust utilities.

use nvim_oxi::Dictionary;
use nvim_oxi::Function;
use nvim_oxi::Object;

use crate::cli_flags::CliFlags;

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

/// The main plugin function that returns a [`Dictionary`] of Lua functions exposed to Neovim.
#[nvim_oxi::plugin]
fn rua() -> Dictionary {
    Dictionary::from_iter([
        (
            "format_diagnostic",
            Object::from(Function::from_fn(diagnostics::formatter::format)),
        ),
        (
            "sort_diagnostics",
            Object::from(Function::from_fn(diagnostics::sorter::sort)),
        ),
        (
            "filter_diagnostics",
            Object::from(Function::from_fn(diagnostics::filter::filter)),
        ),
        ("draw_statusline", Object::from(Function::from_fn(statusline::draw))),
        ("draw_statuscolumn", Object::from(Function::from_fn(statuscolumn::draw))),
        ("create_fkr_cmds", Object::from(Function::from_fn(fkr::create_cmds))),
        (
            "get_fd_cli_flags",
            Object::from(Function::from_fn(cli_flags::fd::FdCliFlags.get())),
        ),
        (
            "get_rg_cli_flags",
            Object::from(Function::from_fn(cli_flags::rg::RgCliFlags.get())),
        ),
        ("run_test", Object::from(Function::from_fn(test_runner::run_test))),
    ])
}
