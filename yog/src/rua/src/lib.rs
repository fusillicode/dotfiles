use mlua::prelude::*;

mod diagnostics;
mod fkr_generator;
mod statuscolumn;
mod statusline;
mod utils;

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
    Ok(exports)
}
