use mlua::prelude::*;

use crate::globs::EXCLUSIONS;

/// Returns rg CLI flags as a [`LuaTable`].
pub fn get(lua: &Lua, _: Option<LuaString>) -> LuaResult<LuaTable> {
    let base_opts = [
        "--color never",
        "--column",
        "--hidden",
        "--line-number",
        "--no-heading",
        "--smart-case",
        "--with-filename",
    ];
    let glob_opts = EXCLUSIONS
        .into_iter()
        .map(|glob| format!("--glob !'{glob}'"));
    lua.create_sequence_from(base_opts.into_iter().map(Into::into).chain(glob_opts))
}
