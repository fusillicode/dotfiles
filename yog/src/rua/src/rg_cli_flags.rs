use mlua::prelude::*;

use crate::globs::EXCLUSIONS;

pub const BASE_FLAGS: [&str; 7] = [
    "--color never",
    "--column",
    "--hidden",
    "--line-number",
    "--no-heading",
    "--smart-case",
    "--with-filename",
];

/// Returns rg CLI flags as a [`LuaTable`].
pub fn get(lua: &Lua, _: Option<LuaString>) -> LuaResult<LuaTable> {
    let glob_flags = EXCLUSIONS
        .into_iter()
        .map(|glob| format!("--glob !'{glob}'"));
    lua.create_sequence_from(BASE_FLAGS.into_iter().map(Into::into).chain(glob_flags))
}
