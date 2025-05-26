use std::collections::HashMap;

use mlua::prelude::*;

use crate::diagnostics::filters::DiagnosticsFilter;

/// Filters out diagnostics related to buffers containing the supplied path, lsp source and unwanted messages.
pub struct MsgsBlacklistFilter {
    pub buf_path: String,
    pub blacklist: HashMap<String, Vec<String>>,
}

impl DiagnosticsFilter for MsgsBlacklistFilter {
    fn keep_diagnostic(&self, buf_path: &str, lsp_diag: &LuaTable) -> LuaResult<bool> {
        if !buf_path.contains(&self.buf_path) {
            return Ok(true);
        }
        let Some(blacklist) = self.blacklist.get(&lsp_diag.get::<String>("source")?) else {
            return Ok(true);
        };
        let msg: String = lsp_diag.get("message")?;
        if blacklist.iter().any(|b| msg.to_lowercase().contains(b)) {
            return Ok(false);
        }
        Ok(true)
    }
}

pub fn configured_filters() -> Vec<Box<dyn DiagnosticsFilter>> {
    let common_blacklist = vec![(
        "typos".into(),
        vec![
            "`calle` should be".into(),
            "`producto` should be".into(),
            "`emision` should be".into(),
            "`clase` should be".into(),
        ],
    )]
    .into_iter()
    .collect::<HashMap<_, _>>();

    vec![
        Box::new(MsgsBlacklistFilter {
            buf_path: "es-be".into(),
            blacklist: common_blacklist.clone(),
        }),
        Box::new(MsgsBlacklistFilter {
            buf_path: "yog".into(),
            blacklist: common_blacklist.clone(),
        }),
    ]
}
