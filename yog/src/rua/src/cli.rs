use mlua::prelude::*;

pub mod fd;
pub mod rg;

type ArityOneLuaFunction<'a, O> = Box<dyn Fn(&Lua, Option<LuaString>) -> LuaResult<O> + 'a>;

pub const GLOB_BLACKLIST: [&str; 6] = [
    "**/.git/*",
    "**/target/*",
    "**/_build/*",
    "**/deps/*",
    "**/.elixir_ls/*",
    "**/node_modules/*",
];

pub trait Flags {
    fn base_flags() -> Vec<&'static str>;

    fn glob_flag(glob: &str) -> String;

    fn get(&self) -> ArityOneLuaFunction<'_, LuaTable> {
        Box::new(|lua: &Lua, _: Option<LuaString>| {
            lua.create_sequence_from(
                Self::base_flags()
                    .into_iter()
                    .map(Into::into)
                    .chain(GLOB_BLACKLIST.into_iter().map(Self::glob_flag)),
            )
        })
    }
}
