use mlua::chunk;
use mlua::prelude::*;

/// Extracts a field of type T from deeply nested [`LuaTable`]s.
/// Similar to [vim.tbl_get()](https://neovim.io/doc/user/lua.html#vim.tbl_get()).
pub fn dig<T: FromLua>(tbl: &LuaTable, keys: &[&str]) -> Result<T, DigError> {
    match keys {
        [] => Err(DigError::NoKeysSupplied),
        [leaf] => tbl.raw_get::<T>(*leaf).map_err(DigError::ConversionError),
        [path @ .., leaf] => {
            let mut tbl = tbl.to_owned();
            for key in path {
                tbl = tbl
                    .raw_get::<LuaTable>(*key)
                    .map_err(|e| DigError::KeyNotFound(key.to_string(), e))?;
            }
            tbl.raw_get::<T>(*leaf).map_err(DigError::ConversionError)
        }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum DigError {
    #[error("no keys supplied")]
    NoKeysSupplied,
    #[error("key {0:?} not found, error {1:?}")]
    KeyNotFound(String, mlua::Error),
    #[error("type conversion error {0:?}")]
    ConversionError(mlua::Error),
}

impl From<DigError> for mlua::Error {
    fn from(value: DigError) -> Self {
        mlua::Error::external(value)
    }
}

/// Print debug Rust constructed values directly into NVim.
#[allow(dead_code)]
pub fn ndbg<T: mlua::IntoLua>(lua: &Lua, value: T) -> mlua::Result<()> {
    lua.load(chunk! { return function(tbl) print(vim.inspect(tbl)) end })
        .eval::<mlua::Function>()?
        .call::<()>(value)
}
