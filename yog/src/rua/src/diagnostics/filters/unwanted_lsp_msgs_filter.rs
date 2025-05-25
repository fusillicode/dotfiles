use std::collections::HashMap;

use mlua::prelude::*;

use crate::diagnostics::filters::DiagnosticsFilter;

/// Filters out diagnostics related to buffers containing the supplied path, lsp source and unwanted messages.
pub struct UnwantedLspMsgsFilter {
    pub buf_path: String,
    pub lsp_unwanted_msgs: HashMap<String, Vec<String>>,
}

impl DiagnosticsFilter for UnwantedLspMsgsFilter {
    fn keep_diagnostic(&self, buf_path: &str, lsp_diag: &LuaTable) -> LuaResult<bool> {
        if !buf_path.contains(&self.buf_path) {
            return Ok(true);
        }
        let Some(unwanted_msgs) = self
            .lsp_unwanted_msgs
            .get(&lsp_diag.get::<String>("source")?)
        else {
            return Ok(true);
        };
        let lsp_diag_msg: String = lsp_diag.get("message")?;
        if unwanted_msgs
            .iter()
            .any(|x| lsp_diag_msg.to_lowercase().contains(x))
        {
            return Ok(false);
        }
        Ok(true)
    }
}
