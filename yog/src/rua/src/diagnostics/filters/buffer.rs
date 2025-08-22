use mlua::prelude::*;
use utils::system::build_home_path;

use crate::diagnostics::filters::DiagnosticsFilter;

/// Filters out diagnostics based on the coded paths blacklist.
pub struct BufferFilter {
    blacklist: Vec<String>,
}

impl BufferFilter {
    pub fn new() -> Self {
        Self {
            blacklist: Self::paths_blacklist().to_vec(),
        }
    }

    /// List of paths for which I don't want to report any diagnostic.
    fn paths_blacklist() -> [String; 1] {
        [build_home_path(&[".cargo"])
            .unwrap_or_default()
            .to_string_lossy()
            .to_string()]
    }
}

impl DiagnosticsFilter for BufferFilter {
    fn skip_diagnostic(&self, buf_path: &str, _lsp_diag: Option<&LuaTable>) -> LuaResult<bool> {
        Ok(self.blacklist.iter().any(|up| buf_path.contains(up)))
    }
}
