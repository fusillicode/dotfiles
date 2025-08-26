use nvim_oxi::Dictionary;

use crate::cli_flags::CliFlags;

mod cli_flags;
mod diagnostics;
mod fkr;
mod statuscolumn;
mod statusline;

#[nvim_oxi::plugin]
fn rua2() -> Dictionary {
    Dictionary::from_iter([
        ("format_diagnostic", diagnostics::formatter::format()),
        ("sort_diagnostics", diagnostics::sorter::sort()),
        ("draw_statusline", statusline::draw()),
        ("draw_statuscolumn", statuscolumn::draw()),
        ("create_fkr_cmds", fkr::create_cmds()),
        ("get_fd_cli_flags", cli_flags::fd::FdCliFlags.get()),
        ("get_rg_cli_flags", cli_flags::rg::RgCliFlags.get()),
    ])
}
