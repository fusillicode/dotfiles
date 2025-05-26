use mlua::prelude::*;

use crate::diagnostics::filters::msg_blacklist::MsgBlacklistFilter;
use crate::diagnostics::filters::related_info::RelatedInfoFilter;

pub mod buffer;
pub mod msg_blacklist;
pub mod related_info;

pub trait DiagnosticsFilter {
    fn skip_diagnostic(&self, buf_path: &str, lsp_diag: Option<&LuaTable>) -> LuaResult<bool>;
}

pub struct DiagnosticsFilters(Vec<Box<dyn DiagnosticsFilter>>);

impl DiagnosticsFilters {
    // Order of filters is IMPORTANT.
    pub fn all(lsp_diags: &LuaTable) -> LuaResult<Self> {
        let mut tmp = MsgBlacklistFilter::all();
        tmp.push(Box::new(RelatedInfoFilter::new(lsp_diags)?));
        Ok(DiagnosticsFilters(tmp))
    }
}

impl DiagnosticsFilter for DiagnosticsFilters {
    fn skip_diagnostic(&self, buf_path: &str, lsp_diag: Option<&LuaTable>) -> LuaResult<bool> {
        // The first filter that returns true skips the LSP diagnostic and all subsequent filters
        // evaluation.
        for filter in &self.0 {
            if filter.skip_diagnostic(buf_path, lsp_diag)? {
                return Ok(true);
            }
        }
        Ok(false)
    }
}
