use nvim_oxi::Dictionary;
use nvim_oxi::api::types::LogLevel;

use crate::cli_flags::CliFlags;

mod cli_flags;
mod diagnostics;
mod fkr;
mod statuscolumn;
mod statusline;
mod test_runner;

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
        ("run_test", test_runner::run_test()),
    ])
}

pub fn log_error(msg: &str) {
    if let Err(error) = nvim_oxi::api::notify(msg, LogLevel::Error, &Default::default()) {
        nvim_oxi::dbg!(format!("fail to notify error {msg:?}, error {error:#?}"));
    }
}

pub fn log_warn(msg: &str) {
    if let Err(error) = nvim_oxi::api::notify(msg, LogLevel::Warn, &Default::default()) {
        nvim_oxi::dbg!(format!("fail to notify warning {msg:?}, error {error:#?}"));
    }
}
