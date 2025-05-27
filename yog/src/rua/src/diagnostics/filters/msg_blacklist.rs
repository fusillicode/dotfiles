use std::collections::HashMap;

use mlua::prelude::*;

use crate::diagnostics::filters::DiagnosticsFilter;

/// Filters out diagnostics related to buffers containing the supplied path, lsp source and unwanted messages.
pub struct MsgBlacklistFilter {
    pub buf_path: String,
    pub blacklist: HashMap<String, Vec<String>>,
}

impl MsgBlacklistFilter {
    pub fn all() -> Vec<Box<dyn DiagnosticsFilter>> {
        Self::typos_lsp_msg_filters()
    }

    fn typos_lsp_msg_filters() -> Vec<Box<dyn DiagnosticsFilter>> {
        let typos_common_blacklist = vec![(
            "typos".into(),
            [
                "anual",
                "calle",
                "clase",
                "clea",
                "cliente",
                "constituye",
                "emision",
                "foto",
                "importante",
                "informativo",
                "momento",
                "producto",
            ]
            .iter()
            .map(|term| format!("`{term}` should be"))
            .collect(),
        )]
        .into_iter()
        .collect::<HashMap<_, _>>();

        vec![
            Box::new(MsgBlacklistFilter {
                buf_path: "/es-be/".into(),
                blacklist: typos_common_blacklist.clone(),
            }),
            Box::new(MsgBlacklistFilter {
                buf_path: "/yog/".into(),
                blacklist: typos_common_blacklist,
            }),
        ]
    }
}

impl DiagnosticsFilter for MsgBlacklistFilter {
    fn skip_diagnostic(&self, buf_path: &str, lsp_diag: Option<&LuaTable>) -> LuaResult<bool> {
        let Some(lsp_diag) = lsp_diag else {
            return Ok(false);
        };
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
