use mlua::prelude::*;

mod diagnostics;
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
    Ok(exports)
}
