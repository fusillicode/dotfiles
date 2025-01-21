use mlua::prelude::*;

use crate::diagnostics::Diagnostic;

/// Returns the formatted [`String`] representation of an LSP diagnostic.
/// The fields of the LSP diagnostic are extracted 1 by 1 from its supplied [`LuaTable`] representation.
pub fn format_diagnostic(_lua: &Lua, diag: Diagnostic) -> LuaResult<String> {
    let msg = get_msg(&diag).map_or_else(
        || format!("no message in {diag:?}"),
        |s| s.trim_end_matches('.').to_string(),
    );
    let src = get_src(&diag).map_or_else(|| format!("no source in {diag:?}"), str::to_string);
    let code = get_code(&diag);
    let src_and_code = code.map_or_else(|| src.clone(), |c| format!("{src}: {c}"));

    Ok(format!("▶ {msg} [{src_and_code}]"))
}

/// Extracts LSP diagnostic message from [`LspData`] `rendered` or directly from the supplied [`Diagnostic`]
fn get_msg(diag: &Diagnostic) -> Option<&str> {
    diag.user_data
        .as_ref()
        .and_then(|user_data| {
            user_data
                .lsp
                .as_ref()
                .and_then(|lsp| {
                    lsp.data
                        .as_ref()
                        .and_then(|lsp_data| lsp_data.rendered.as_deref())
                })
                .or(user_data.message.as_deref())
        })
        .or(diag.message.as_deref())
}

/// Extracts LSP diagnostic source and code from `user_data.lsp.data` or just `source` or directly from the supplied [`LuaTable`]
fn get_src(diag: &Diagnostic) -> Option<&str> {
    diag.user_data
        .as_ref()
        .and_then(|user_data| user_data.lsp.as_ref().and_then(|lsp| lsp.source.as_deref()))
        .or(diag.source.as_deref())
}

/// Extracts LSP diagnostic source and code from `user_data.lsp.data` or just `source` or directly from the supplied [`LuaTable`]
fn get_code(diag: &Diagnostic) -> Option<&str> {
    diag.user_data
        .as_ref()
        .and_then(|user_data| user_data.lsp.as_ref().and_then(|lsp| lsp.code.as_deref()))
        .or(diag.code.as_deref())
}
