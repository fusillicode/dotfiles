use mlua::prelude::*;

use crate::diagnostics::filters::DiagnosticsFilter;

/// Filters out diagnostics already represented by other ones
/// (e.g. HINTs pointing to a location already mentioned by other ERROR's rendered message)
pub struct TyposLspFilter;

impl DiagnosticsFilter for TyposLspFilter {
    fn keep(&self, buf_path: &str, lsp_diag: &LuaTable) -> LuaResult<bool> {
        if lsp_diag.get::<String>("source")? != "typos" {
            return Ok(true);
        }
        if buf_path.contains("es-be") {
            let msg: String = lsp_diag.get("message")?;
            if msg.to_lowercase().contains("`calle` should be") {
                return Ok(false);
            }
        }
        Ok(true)
    }
}
