use std::collections::HashMap;

use mlua::prelude::*;

use crate::diagnostics::filters::DiagnosticsFilter;

/// Filters out diagnostics related to buffers containing the supplied path, lsp source and unwanted messages.
pub struct MsgBlacklistFilter {
    pub buf_path: String,
    pub blacklist: HashMap<String, Vec<String>>,
}

impl DiagnosticsFilter for MsgBlacklistFilter {
    fn skip_diagnostic(&self, buf_path: &str, lsp_diag: &LuaTable) -> LuaResult<bool> {
        if !buf_path.contains(&self.buf_path) {
            return Ok(false);
        }
        let Some(blacklist) = self.blacklist.get(&lsp_diag.get::<String>("source")?) else {
            return Ok(false);
        };
        let msg = lsp_diag.get::<String>("message")?.to_lowercase();
        if blacklist.iter().any(|b| msg.contains(b)) {
            return Ok(true);
        }
        Ok(false)
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
        Box::new(MsgBlacklistFilter {
            buf_path: "/es-be/".into(),
            blacklist: common_blacklist.clone(),
        }),
        Box::new(MsgBlacklistFilter {
            buf_path: "/yog/".into(),
            blacklist: common_blacklist,
        }),
    ]
}
