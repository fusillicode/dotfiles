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

    for (lua_fn_name, rust_fn) in [
        (
            "format_diagnostic",
            lua.create_function(diagnostics::formatter::format)?,
        ),
        (
            "filter_diagnostics",
            lua.create_function(diagnostics::filter::filter)?,
        ),
        ("draw_statusline", lua.create_function(statusline::draw)?),
        (
            "draw_statuscolumn",
            lua.create_function(statuscolumn::draw)?,
        ),
        ("get_fkr_cmds", lua.create_function(fkr_gen::get_cmds)?),
        ("gen_fkr_value", lua.create_function(fkr_gen::gen_value)?),
        (
            "get_fd_cli_flags",
            lua.create_function(cli::fd::CliFlags.get())?,
        ),
        (
            "get_rg_cli_flags",
            lua.create_function(cli::rg::CliFlags.get())?,
        ),
        ("run_test", lua.create_function(test_runner::run_test)?),
    ] {
        exports.set(lua_fn_name, rust_fn)?;
    }

    Ok(exports)
}
