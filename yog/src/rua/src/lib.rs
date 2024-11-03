use mlua::chunk;
use mlua::prelude::*;
use serde::Serialize;

#[mlua::lua_module]
fn rua(lua: &Lua) -> LuaResult<LuaTable> {
    let exports = lua.create_table()?;
    exports.set("format_diagnostic", lua.create_function(format_diagnostic)?)?;
    exports.set(
        "filter_diagnostics",
        lua.create_function(filter_diagnostics)?,
    )?;
    Ok(exports)
}

fn format_diagnostic(_lua: &Lua, lsp_diag: LuaTable) -> LuaResult<String> {
    let lsp_data = dig::<LuaTable>(&lsp_diag, &["user_data", "lsp"])?;

    let diag_msg = dig::<String>(&lsp_data, &["data", "rendered"])
        .or_else(|_| dig::<String>(&lsp_data, &["message"]))
        .map(|s| s.trim_end_matches('.').to_owned())?;

    let src_and_code = match (
        dig::<String>(&lsp_data, &["source"])
            .map(|s| s.trim_end_matches('.').to_owned())
            .ok(),
        dig::<String>(&lsp_data, &["code"])
            .map(|s| s.trim_end_matches('.').to_owned())
            .ok(),
    ) {
        (None, None) => None,
        (Some(src), None) => Some(src),
        (None, Some(code)) => Some(code),
        (Some(src), Some(code)) => Some(format!("{src}: {code}")),
    }
    .map(|s| format!(" [{s}]"))
    .unwrap_or_else(String::new);

    Ok(format!("▶ {diag_msg}{src_and_code}"))
}

fn filter_diagnostics(lua: &Lua, lsp_diags: LuaTable) -> LuaResult<LuaTable> {
    let lsp_diags_vec = lsp_diags
        .sequence_values::<LuaTable>()
        .flatten()
        .collect::<Vec<_>>();
    let rel_info_diags = get_related_info_diag(&lsp_diags_vec)?;
    ndbg(lua, lua.to_value(&rel_info_diags).unwrap()).unwrap();
    Ok(lsp_diags)
}

fn get_related_info_diag(lsp_diags: &[LuaTable]) -> LuaResult<Vec<RelatedInfoDiag>> {
    let mut rel_diags = vec![];
    for lsp_diag in lsp_diags {
        let Ok(rel_infos) = dig::<LuaTable>(lsp_diag, &["user_data", "lsp", "relatedInformation"])
        else {
            continue;
        };
        for rel_info in rel_infos.sequence_values::<LuaTable>().flatten() {
            let msg = dig::<String>(&rel_info, &["message"])?;
            let start = dig::<LuaTable>(&rel_info, &["location", "range", "start"])?;
            let end = dig::<LuaTable>(&rel_info, &["location", "range", "end"])?;
            rel_diags.push(RelatedInfoDiag {
                msg,
                start: Pos {
                    ln: dig::<usize>(&start, &["line"])?,
                    col: dig::<usize>(&start, &["character"])?,
                },
                end: Pos {
                    ln: dig::<usize>(&end, &["line"])?,
                    col: dig::<usize>(&end, &["character"])?,
                },
            });
        }
    }
    Ok(rel_diags)
}

#[derive(Debug, PartialEq, Eq, Clone, Serialize)]
struct RelatedInfoDiag {
    msg: String,
    start: Pos,
    end: Pos,
}

#[derive(Debug, PartialEq, Eq, Clone, Serialize)]
struct Pos {
    ln: usize,
    col: usize,
}

fn dig<T: FromLua>(tbl: &LuaTable, keys: &[&str]) -> Result<T, DigError> {
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
enum DigError {
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

#[allow(dead_code)]
fn ndbg<T: mlua::IntoLua>(lua: &Lua, value: T) -> mlua::Result<()> {
    lua.load(chunk! { return function(tbl) print(vim.inspect(tbl)) end })
        .eval::<mlua::Function>()?
        .call::<()>(value)
}
