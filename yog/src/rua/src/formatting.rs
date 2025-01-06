use mlua::prelude::*;

use crate::utils::dig;
use crate::utils::DigError::KeyNotFound;

/// Returns the formatted [`String`] representation of an LSP diagnostic.
/// The fields of the LSP diagnostic are extracted 1 by 1 from its supplied [`LuaTable`] representation.
pub fn format_diagnostic(_lua: &Lua, lsp_diag: LuaTable) -> LuaResult<String> {
    let msg = get_msg(&lsp_diag)?;
    let src_and_code = get_src_and_code(&lsp_diag)?;
    Ok(format!("▶ {msg} [{src_and_code}]"))
}

/// Extract LSP diagnostic message from `user_data.lsp.data.rendered` or directly from the supplied [`LuaTable`]
fn get_msg(lsp_diag: &LuaTable) -> LuaResult<String> {
    Ok(dig::<LuaTable>(lsp_diag, &["user_data", "lsp"])
        .and_then(|x| {
            dig::<String>(&x, &["data", "rendered"]).or_else(|_| dig::<String>(&x, &["message"]))
        })
        .or_else(|_| dig::<String>(lsp_diag, &["message"]))
        .map(|s| s.trim_end_matches('.').to_owned())?)
}

/// Extract LSP diagnostic source and code from `user_data.lsp.data` or just `source` or directly from the supplied [`LuaTable`]
fn get_src_and_code(lsp_diag: &LuaTable) -> LuaResult<String> {
    Ok(dig::<LuaTable>(lsp_diag, &["user_data", "lsp"])
        .and_then(|x| {
            match (
                dig::<String>(&x, &["source"])
                    .map(|s| s.trim_end_matches('.').to_owned())
                    .ok(),
                dig::<String>(&x, &["code"])
                    .map(|s| s.trim_end_matches('.').to_owned())
                    .ok(),
            ) {
                (None, None) => Err(KeyNotFound(
                    "source_and_code".into(),
                    mlua::Error::runtime("user_data.lsp.{source|code} keys not found"),
                )),
                (Some(src), None) => Ok(src),
                (None, Some(code)) => Ok(code),
                (Some(src), Some(code)) => Ok(format!("{src}: {code}")),
            }
        })
        .or_else(|_| dig::<String>(lsp_diag, &["source"]))?)
}
