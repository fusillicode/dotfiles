use mlua::prelude::*;

mod filtering;
mod formatting;
mod utils;

/// Entrypoint of Rust exported fns.
#[mlua::lua_module]
fn rua(lua: &Lua) -> LuaResult<LuaTable> {
    let exports = lua.create_table()?;
    exports.set(
        "format_diagnostic",
        lua.create_function(formatting::format_diagnostic)?,
    )?;
    exports.set(
        "filter_diagnostics",
        lua.create_function(filtering::filter_diagnostics)?,
    )?;
    Ok(exports)
}
