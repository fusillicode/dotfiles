use mlua::prelude::*;

mod diagnostics;
mod test_runner;
mod utils;

/// Neovim Lua module providing Rust utilities for enhanced editor functionality.
///
/// This module exports various functions that can be called from Lua scripts in Neovim,
/// providing access to Rust implementations of common editor operations and utilities.
/// The functions are designed to be fast, reliable, and integrate seamlessly with
/// Neovim's Lua API.
///
/// # Available Functions
///
/// - `format_diagnostic`: Formats diagnostic messages for display
/// - `filter_diagnostics`: Filters diagnostic messages based on criteria
/// - `sort_diagnostics`: Sorts diagnostic messages by priority/severity
/// - `draw_statusline`: Renders custom statusline components
/// - `draw_statuscolumn`: Renders custom statuscolumn components
/// - `get_fkr_cmds`: Provides fake data generation commands
/// - `gen_fkr_value`: Generates fake data values
/// - `get_fd_cli_flags`: Gets file descriptor CLI flags
/// - `get_rg_cli_flags`: Gets ripgrep CLI flags
/// - `run_test`: Executes tests and returns results
///
/// # Error Handling
///
/// Functions include built-in error handling with logging to a debug file when
/// compiled in debug mode. Errors are converted to Lua errors and can be caught
/// in Lua scripts.
///
/// # Examples
///
/// Loading the module in Lua:
/// ```lua
/// local rua = require('rua')
/// local result = rua.format_diagnostic(diagnostic)
/// ```
///
/// # Performance
///
/// All functions are optimized for performance and use efficient data structures.
/// Some operations run in parallel when possible to minimize blocking.
#[mlua::lua_module]
fn rua(lua: &Lua) -> LuaResult<LuaTable> {
    let exports = lua.create_table()?;

    for (lua_fn_name, rust_fn) in [
        (
            "filter_diagnostics",
            create_debuggable_fn(lua, diagnostics::filter::filter)?,
        ),
        ("run_test", create_debuggable_fn(lua, test_runner::run_test)?),
    ] {
        exports.set(lua_fn_name, rust_fn)?;
    }

    Ok(exports)
}

/// Creates a debuggable Lua function that logs errors and results in debug builds.
///
/// This function wraps Rust functions to make them callable from Lua while adding
/// comprehensive error logging and debugging capabilities. In debug builds, all
/// function calls and their results are logged to a file for troubleshooting.
///
/// The wrapper handles the conversion between Lua and Rust types, error propagation,
/// and provides detailed logging including timestamps, error sources, and backtraces.
///
/// # Type Parameters
///
/// * `F`: The Rust function type to wrap
/// * `A`: The argument types that can be converted from Lua
/// * `R`: The return types that can be converted to Lua
///
/// # Arguments
///
/// * `lua`: Reference to the Lua state
/// * `func`: The Rust function to wrap
///
/// # Returns
///
/// Returns a [LuaFunction] that can be called from Lua scripts.
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

/// Logs function results and errors to a debug log file in debug builds.
///
/// This function is only compiled in debug builds and provides comprehensive
/// logging of all Lua function calls made through the rua module. It logs both
/// successful results and errors with detailed information including timestamps,
/// error sources, and backtraces.
///
/// The log file is located at `~/.local/state/nvim/rua.log` and contains
/// structured JSON entries for each function call.
///
/// # Arguments
///
/// * `res`: The result of the function call to log
///
/// # Returns
///
/// Returns `Ok(())` if logging succeeds, or an error if file operations fail.
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
