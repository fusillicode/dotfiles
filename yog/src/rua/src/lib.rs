use mlua::prelude::*;

use crate::cli::Flags;

mod cli;
mod diagnostics;
mod fkr_gen;
mod statuscolumn;
mod statusline;
mod test_runner;
mod utils;

/// Entrypoint of Rust exported fns.
#[mlua::lua_module]
fn rua(lua: &Lua) -> LuaResult<LuaTable> {
    let exports = lua.create_table()?;
    exports.set(
        "format_diagnostic",
        lua.create_function(diagnostics::formatter::format)?,
    )?;
    exports.set(
        "filter_diagnostics",
        lua.create_function(diagnostics::filter::filter)?,
    )?;
    exports.set("draw_statusline", lua.create_function(statusline::draw)?)?;
    exports.set(
        "draw_statuscolumn",
        lua.create_function(statuscolumn::draw)?,
    )?;
    exports.set("get_fkr_cmds", lua.create_function(fkr_gen::get_cmds)?)?;
    exports.set("gen_fkr_value", lua.create_function(fkr_gen::gen_value)?)?;
    exports.set(
        "get_fd_cli_flags",
        lua.create_function(cli::fd::CliFlags.get())?,
    )?;
    exports.set(
        "get_rg_cli_flags",
        lua.create_function(cli::rg::CliFlags.get())?,
    )?;
    exports.set("run_test", lua.create_function(test_runner::run_test)?)?;
    Ok(exports)
}
