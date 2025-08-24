use mlua::prelude::*;

use crate::cli::Flags;

mod cli;
mod diagnostics;
mod fkr_gen;
mod statuscolumn;
mod statusline;
mod test_runner;
mod utils;

/// Entrypoint of Rust exported functions.
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
    F: Fn(&Lua, A) -> Result<R, anyhow::Error> + 'a + 'static,
    A: FromLuaMulti,
    R: IntoLuaMulti + std::fmt::Debug,
{
    lua.create_function(move |lua, args: A| {
        let res = func(lua, args);
        #[cfg(debug_assertions)]
        log_result(&res)?;
        res.map_err(mlua::Error::from)
    })
}

#[cfg(debug_assertions)]
fn log_result<R: IntoLuaMulti + std::fmt::Debug>(res: &Result<R, anyhow::Error>) -> anyhow::Result<()> {
    use std::fs::OpenOptions;
    use std::io::Write;

    use anyhow::anyhow;

    let log_path = ::utils::system::build_home_path(&[".local", "state", "nvim", "rua.log"]).map_err(|e| anyhow!(e))?;
    let mut log_file = OpenOptions::new().append(true).create(true).open(log_path)?;

    let now = chrono::Utc::now();
    let log_msg = res.as_ref().map_or_else(
        |error| {
            serde_json::json!({
                "timestamp": now,
                "error": format!("{error:#?}"),
                "source": format!("{:#?}", error.source()),
                "backtrace": format!("{:#?}", error.backtrace())
            })
        },
        |r| {
            serde_json::json!({
                "timestamp": now,
                "result": format!("{r:#?}"),
            })
        },
    );
    writeln!(log_file, "{log_msg}")?;

    Ok(())
}
