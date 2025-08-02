use mlua::prelude::*;

use crate::globs::EXCLUSIONS;

pub const BASE_FLAGS: [&str; 5] = [
    "--color never",
    "--follow",
    "--hidden",
    "--no-ignore-vcs",
    "--type f",
];

/// Returns fd CLI flags as a [`LuaTable`].
pub fn get(lua: &Lua, _: Option<LuaString>) -> LuaResult<LuaTable> {
    let glob_flags = EXCLUSIONS
        .into_iter()
        .map(|glob| format!("--exclude '{glob}'"));
    lua.create_sequence_from(BASE_FLAGS.into_iter().map(Into::into).chain(glob_flags))
}
