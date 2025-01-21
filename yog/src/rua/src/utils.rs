use mlua::chunk;
use mlua::prelude::*;

/// Print debug Rust constructed values directly into NVim.
#[allow(dead_code)]
pub fn ndbg<T: mlua::IntoLua>(lua: &Lua, value: T) -> mlua::Result<()> {
    lua.load(chunk! { return function(tbl) print(vim.inspect(tbl)) end })
        .eval::<mlua::Function>()?
        .call::<()>(value)
}
