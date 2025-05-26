use mlua::prelude::*;

pub mod buffers;
pub mod msg_blacklist;
pub mod related_info;

pub trait DiagnosticsFilter {
    fn skip_diagnostic(&self, buf_path: &str, lsp_diag: &LuaTable) -> LuaResult<bool>;
}
