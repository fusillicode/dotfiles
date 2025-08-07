use mlua::prelude::*;

use crate::cli::Flags;

mod cli;
mod diagnostics;
mod fd;
mod fkr_generator;
mod rg;
mod statuscolumn;
mod statusline;
mod test_runner;
mod utils;

type ArityOneLuaFunction<'a, O> = Box<dyn Fn(&Lua, Option<LuaString>) -> LuaResult<O> + 'a>;

/// Entrypoint of Rust exported fns.
#[mlua::lua_module]
fn rua(lua: &Lua) -> LuaResult<LuaTable> {
    let exports = lua.create_table()?;
    exports.set(
        "format_diagnostic",
        lua.create_function(diagnostics::formatting::format_diagnostic)?,
    )?;
    exports.set(
        "filter_diagnostics",
        lua.create_function(diagnostics::filtering::filter_diagnostics)?,
    )?;
    exports.set("draw_statusline", lua.create_function(statusline::draw)?)?;
    exports.set(
        "draw_statuscolumn",
        lua.create_function(statuscolumn::draw)?,
    )?;
    exports.set(
        "get_fkr_cmds",
        lua.create_function(fkr_generator::get_cmds)?,
    )?;
    exports.set(
        "gen_fkr_value",
        lua.create_function(fkr_generator::gen_value)?,
    )?;
    exports.set("get_fd_cli_flags", lua.create_function(fd::CliFlags.get())?)?;
    exports.set("get_rg_cli_flags", lua.create_function(rg::CliFlags.get())?)?;
    exports.set("run_test", lua.create_function(test_runner::run_test)?)?;
    Ok(exports)
}
