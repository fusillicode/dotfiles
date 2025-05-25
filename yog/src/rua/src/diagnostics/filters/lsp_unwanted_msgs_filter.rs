use mlua::prelude::*;

use crate::diagnostics::filters::DiagnosticsFilter;

/// Filters out diagnostics already represented by other ones
pub struct LspUnwantedMsgsFilter {
    pub lsp: String,
    pub buf_path: String,
    pub unwanted_msgs: Vec<String>,
}

impl DiagnosticsFilter for LspUnwantedMsgsFilter {
    fn apply(&self, out: &mut Vec<LuaTable>, buf_path: &str, lsp_diag: LuaTable) -> LuaResult<()> {
        if lsp_diag.get::<String>("source")? != self.lsp {
            return Ok(());
        }
        if buf_path.contains(&self.buf_path) {
            let msg: String = lsp_diag.get("message")?;
            if self
                .unwanted_msgs
                .iter()
                .any(|u_msg| msg.to_lowercase().contains(u_msg))
            {
                return Ok(());
            }
        }
        out.push(lsp_diag);
        Ok(())
    }
}
