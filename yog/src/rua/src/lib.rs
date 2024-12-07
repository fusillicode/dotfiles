use mlua::chunk;
use mlua::prelude::*;
use serde::Serialize;

/// Entrypoint of Rust exported fns.
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

/// Returns the formatted [`String`] representation of an LSP diagnostic.
/// The fields of the LSP diagnostic are extracted 1 by 1 from its supplied [`LuaTable`] representation.
pub fn format_diagnostic(_lua: &Lua, lsp_diag: LuaTable) -> LuaResult<String> {
    let (diag_msg, src_and_code) = dig::<LuaTable>(&lsp_diag, &["user_data", "lsp"])
        .ok()
        .as_ref()
        .map_or_else(
            || diag_msg_and_source_code_from_lsp_diag(&lsp_diag),
            diag_msg_and_source_from_lsp_user_data,
        )?;
    Ok(format!("▶ {diag_msg}{src_and_code}"))
}

/// Filter out the LSP diagnostics that are already represented by other ones, e.g. HINTs pointing
/// to location already mentioned by other ERROR's rendered message.
pub fn filter_diagnostics(lua: &Lua, lsp_diags: LuaTable) -> LuaResult<LuaTable> {
    let rel_info_diags = get_related_info_diag(&lsp_diags)?;
    if rel_info_diags.is_empty() {
        return Ok(lsp_diags);
    }
    let out = lua.create_table()?;
    for lsp_diag in lsp_diags.sequence_values::<LuaTable>().flatten() {
        let rel = RelatedInfoDiag {
            msg: dig::<String>(&lsp_diag, &["message"])?,
            start: Pos {
                ln: dig::<usize>(&lsp_diag, &["lnum"])?,
                col: dig::<usize>(&lsp_diag, &["col"])?,
            },
            end: Pos {
                ln: dig::<usize>(&lsp_diag, &["end_lnum"])?,
                col: dig::<usize>(&lsp_diag, &["end_col"])?,
            },
        };
        if !rel_info_diags.contains(&rel) {
            out.push(lsp_diag)?;
        }
    }
    Ok(out)
}

/// Extract "dialog message" plus formatted "source and code" from the root LSP diagnostic.
fn diag_msg_and_source_code_from_lsp_diag(lsp_diag: &LuaTable) -> LuaResult<(String, String)> {
    Ok((
        dig::<String>(lsp_diag, &["message"])?,
        format_src_and_code(&dig::<String>(lsp_diag, &["source"])?),
    ))
}

/// Extract "dialog message" plus formatted "source and code" from the LSP user data.
fn diag_msg_and_source_from_lsp_user_data(lsp_user_data: &LuaTable) -> LuaResult<(String, String)> {
    let diag_msg = dig::<String>(lsp_user_data, &["data", "rendered"])
        .or_else(|_| dig::<String>(lsp_user_data, &["message"]))
        .map(|s| s.trim_end_matches('.').to_owned())?;

    let src_and_code = match (
        dig::<String>(lsp_user_data, &["source"])
            .map(|s| s.trim_end_matches('.').to_owned())
            .ok(),
        dig::<String>(lsp_user_data, &["code"])
            .map(|s| s.trim_end_matches('.').to_owned())
            .ok(),
    ) {
        (None, None) => None,
        (Some(src), None) => Some(src),
        (None, Some(code)) => Some(code),
        (Some(src), Some(code)) => Some(format!("{src}: {code}")),
    }
    .map(|src_and_code| format_src_and_code(&src_and_code))
    .unwrap_or_else(String::new);

    Ok((diag_msg, src_and_code))
}

/// Format the supplied `[&str]` as the expected "source and code".
fn format_src_and_code(src_and_code: &str) -> String {
    format!(" [{src_and_code}]")
}

/// Get the message and posisiton of the LSP "relatedInformation"s inside LSP diagnostics.
fn get_related_info_diag(lsp_diags: &LuaTable) -> LuaResult<Vec<RelatedInfoDiag>> {
    let mut rel_diags = vec![];
    for lsp_diag in lsp_diags.sequence_values::<LuaTable>().flatten() {
        let Ok(rel_infos) = dig::<LuaTable>(&lsp_diag, &["user_data", "lsp", "relatedInformation"])
        else {
            continue;
        };
        for rel_info in rel_infos.sequence_values::<LuaTable>().flatten() {
            let start = dig::<LuaTable>(&rel_info, &["location", "range", "start"])?;
            let end = dig::<LuaTable>(&rel_info, &["location", "range", "end"])?;
            rel_diags.push(RelatedInfoDiag {
                msg: dig::<String>(&rel_info, &["message"])?,
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

/// Utility to extract fields from deep nested [`LuaTable`]s.
/// Similar to [vim.tbl_get()](https://neovim.io/doc/user/lua.html#vim.tbl_get()).
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

/// Utility to print debug Rust constructed values directly into NVIM.
#[allow(dead_code)]
fn ndbg<T: mlua::IntoLua>(lua: &Lua, value: T) -> mlua::Result<()> {
    lua.load(chunk! { return function(tbl) print(vim.inspect(tbl)) end })
        .eval::<mlua::Function>()?
        .call::<()>(value)
}
