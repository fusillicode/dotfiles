use mlua::prelude::*;

pub mod buffer;
pub mod msg_blacklist;
pub mod related_info;

pub trait DiagnosticsFilter {
    fn skip_diagnostic(&self, buf_path: &str, lsp_diag: Option<&LuaTable>) -> LuaResult<bool>;
}

pub struct DiagnosticsFilters(Vec<Box<dyn DiagnosticsFilter>>);

impl DiagnosticsFilters {
    pub fn new(filters: Vec<Box<dyn DiagnosticsFilter>>) -> Self {
        Self(filters)
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
