use mlua::prelude::*;

use crate::LuaFunction;

pub const GLOB_BLACKLIST: [&str; 6] = [
    "**/.git/*",
    "**/target/*",
    "**/_build/*",
    "**/deps/*",
    "**/.elixir_ls/*",
    "**/node_modules/*",
];

pub trait Flags {
    fn base_flags(&self) -> Vec<&str>;

    fn format_glob(&self, glob: &str) -> String;

    fn get(&self) -> LuaFunction<'_> {
        Box::new(|lua: &Lua, _: Option<LuaString>| {
            let glob_flags = GLOB_BLACKLIST
                .into_iter()
                .map(|glob| self.format_glob(glob));
            lua.create_sequence_from(
                self.base_flags()
                    .into_iter()
                    .map(Into::into)
                    .chain(glob_flags),
            )
        })
    }
}
