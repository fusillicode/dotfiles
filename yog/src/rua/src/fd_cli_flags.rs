use mlua::prelude::*;

use crate::globs::EXCLUSIONS;

/// Returns fd CLI flags as a [`LuaTable`].
pub fn get(lua: &Lua, _: Option<LuaString>) -> LuaResult<LuaTable> {
    let base_opts = [
        "--color never",
        "--follow",
        "--hidden",
        "--no-ignore-vcs",
        "--type f",
    ];
    let glob_opts = EXCLUSIONS
        .into_iter()
        .map(|glob| format!("--exclude '{glob}'"));
    lua.create_sequence_from(base_opts.into_iter().map(Into::into).chain(glob_opts))
}
