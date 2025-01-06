use mlua::prelude::*;

use crate::utils::dig;

/// Returns the formatted [`String`] representation of an LSP diagnostic.
/// The fields of the LSP diagnostic are extracted 1 by 1 from its supplied [`LuaTable`] representation.
pub fn format_diagnostic(_lua: &Lua, lsp_diag: LuaTable) -> LuaResult<String> {
    let (diag_msg, src_and_code) = dig::<LuaTable>(&lsp_diag, &["user_data", "lsp"])
        .ok()
        .as_ref()
        .map_or_else(
            || diag_msg_and_source_code_from_lsp_diag(&lsp_diag),
            |lsp_user_data| diag_msg_and_source_from_lsp_user_data(&lsp_diag, lsp_user_data),
        )?;
    Ok(format!("▶ {diag_msg}{src_and_code}"))
}

/// Extract "dialog message" plus formatted "source and code" from the root LSP diagnostic.
fn diag_msg_and_source_code_from_lsp_diag(lsp_diag: &LuaTable) -> LuaResult<(String, String)> {
    Ok((
        dig::<String>(lsp_diag, &["message"])?,
        format_src_and_code(&dig::<String>(lsp_diag, &["source"])?),
    ))
}

/// Extract "dialog message" and the formatted "source and code" from the LSP user data.
/// If the "dialog message" is not in the LSP user data (e.g. `sqlfluff`) fallback to the message in the root
/// "LSP diagnostic".
fn diag_msg_and_source_from_lsp_user_data(
    lsp_diag: &LuaTable,
    lsp_user_data: &LuaTable,
) -> LuaResult<(String, String)> {
    let diag_msg = dig::<String>(lsp_user_data, &["data", "rendered"])
        .or_else(|_| dig::<String>(lsp_user_data, &["message"]))
        .or_else(|_| dig::<String>(lsp_diag, &["message"]))
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

/// Format the supplied [`&str`] as the expected "source and code".
fn format_src_and_code(src_and_code: &str) -> String {
    format!(" [{src_and_code}]")
}
