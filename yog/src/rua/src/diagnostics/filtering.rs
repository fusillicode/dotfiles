use std::collections::HashMap;

use mlua::prelude::*;

use crate::diagnostics::filters::path_filter::no_diagnostics_for_path;
use crate::diagnostics::filters::related_info_filter::RelatedInfoFilter;
use crate::diagnostics::filters::unwanted_lsp_msgs_filter::UnwantedLspMsgsFilter;
use crate::diagnostics::filters::DiagnosticsFilter;

/// Filters out the LSP diagnostics based on the coded filters.
pub fn filter_diagnostics(
    lua: &Lua,
    (buf_path, lsp_diags): (LuaString, LuaTable),
) -> LuaResult<LuaTable> {
    let buf_path = buf_path.to_string_lossy();
    if let Some(out) = no_diagnostics_for_path(lua, &buf_path) {
        return out;
    }

    let filters: Vec<Box<dyn DiagnosticsFilter>> = vec![
        Box::new(RelatedInfoFilter::new(&lsp_diags)?),
        Box::new(UnwantedLspMsgsFilter {
            buf_path: "es-be".into(),
            lsp_unwanted_msgs: vec![(
                "typos".into(),
                vec![
                    "`calle` should be".into(),
                    "producto".into(),
                    "emision".into(),
                    "clase".into(),
                ],
            )]
            .into_iter()
            .collect::<HashMap<_, _>>(),
        }),
    ];

    let mut out = vec![];
    // Using [`.pairs`] and [`LuaValue`] to get a & to the LSP diagnostic [`LuaTable`] and avoid
    // cloning it when passing it to the filters.
    for (_, lua_value) in lsp_diags.pairs::<usize, LuaValue>().flatten() {
        let lsp_diag = lua_value.as_table().ok_or_else(|| {
            mlua::Error::RuntimeError(format!("cannot get LuaTable from LuaValue {lua_value:?}"))
        })?;
        for filter in &filters {
            if filter.keep_diagnostic(&buf_path, lsp_diag)? {
                out.push(lsp_diag.clone());
            }
        }
    }

    lua.create_sequence_from(out)
}
