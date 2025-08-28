use nvim_oxi::Dictionary;

use crate::cli_flags::CliFlags;

mod cli_flags;
mod diagnostics;
mod fkr;
mod oxi_ext;
mod oxi_utils;
mod statuscolumn;
mod statusline;
mod test_runner;

#[nvim_oxi::plugin]
fn rua() -> Dictionary {
    Dictionary::from_iter([
        ("format_diagnostic", diagnostics::formatter::format()),
        ("sort_diagnostics", diagnostics::sorter::sort()),
        ("filter_diagnostics", diagnostics::filter::filter()),
        ("draw_statusline", statusline::draw()),
        ("draw_statuscolumn", statuscolumn::draw()),
        ("create_fkr_cmds", fkr::create_cmds()),
        ("get_fd_cli_flags", cli_flags::fd::FdCliFlags.get()),
        ("get_rg_cli_flags", cli_flags::rg::RgCliFlags.get()),
        ("run_test", test_runner::run_test()),
    ])
}
