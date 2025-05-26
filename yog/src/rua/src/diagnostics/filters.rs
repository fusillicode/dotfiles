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
    // The first filter that returns false skips the LSP diagnostic.
    pub fn all(lsp_diags: &LuaTable) -> LuaResult<Self> {
        let mut tmp = MsgBlacklistFilter::all();
        tmp.push(Box::new(RelatedInfoFilter::new(lsp_diags)?));
        Ok(DiagnosticsFilters(tmp))
    }

    pub fn skip_diagnostic(&self, buf_path: &str, lsp_diag: &LuaTable) -> LuaResult<bool> {
        let mut skip_diagnostic = false;
        for filter in &self.0 {
            if filter.skip_diagnostic(buf_path, Some(lsp_diag))? {
                skip_diagnostic = true;
                break;
            }
        }
        Ok(skip_diagnostic)
    }
}
