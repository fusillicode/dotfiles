use std::fs::OpenOptions;
use std::io::Write;

use anyhow::anyhow;
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
            create_debuggable_fn(lua, diagnostics::formatter::format)?,
        ),
        (
            "filter_diagnostics",
            create_debuggable_fn(lua, diagnostics::filter::filter)?,
        ),
        (
            "sort_diagnostics",
            create_debuggable_fn(lua, diagnostics::sorter::sort)?,
        ),
        ("draw_statusline", create_debuggable_fn(lua, statusline::draw)?),
        ("draw_statuscolumn", create_debuggable_fn(lua, statuscolumn::draw)?),
        ("get_fkr_cmds", create_debuggable_fn(lua, fkr_gen::get_cmds)?),
        ("gen_fkr_value", create_debuggable_fn(lua, fkr_gen::gen_value)?),
        ("get_fd_cli_flags", create_debuggable_fn(lua, cli::fd::CliFlags.get())?),
        ("get_rg_cli_flags", create_debuggable_fn(lua, cli::rg::CliFlags.get())?),
        ("run_test", create_debuggable_fn(lua, test_runner::run_test)?),
    ] {
        exports.set(lua_fn_name, rust_fn)?;
    }

    Ok(exports)
}

// Wrapper function for creating Lua functions that logs to a fixed logfile in case of error and
// debug builds.
fn create_debuggable_fn<'a, F, A, R>(lua: &'a Lua, func: F) -> Result<LuaFunction, mlua::Error>
where
    F: Fn(&Lua, A) -> Result<R, mlua::Error> + 'a + 'static,
    A: FromLuaMulti,
    R: IntoLuaMulti + std::fmt::Debug,
{
    lua.create_function(move |lua, args: A| {
        let res = func(lua, args);
        if cfg!(debug_assertions)
            && let Err(ref error) = res
        {
            write_to_log_file(error)?;
        }
        res
    })
}

fn write_to_log_file<R: std::fmt::Debug>(res: &R) -> anyhow::Result<()> {
    let log_path = ::utils::system::home_path(".local/state/nvim/rua.log").map_err(|e| anyhow!(e))?;
    let mut log_file = OpenOptions::new().append(true).create(true).open(log_path)?;
    writeln!(log_file, "{:#?}", res)?;
    Ok(())
}
