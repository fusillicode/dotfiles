use mlua::prelude::*;

use crate::diagnostics::filters::DiagnosticsFilter;

/// Filters out diagnostics already represented by other ones
/// (e.g. HINTs pointing to a location already mentioned by other ERROR's rendered message)
pub struct RelatedInfoFilter {
    rel_infos: Vec<RelatedInfo>,
}

impl RelatedInfoFilter {
    pub fn new(lsp_diags: &LuaTable) -> LuaResult<Self> {
        Ok(Self {
            rel_infos: Self::get_related_infos(lsp_diags)?,
        })
    }

    /// Get the [`RelatedInfo`]s of an LSP diagnostic represented by a [`LuaTable`].
    fn get_related_infos(lsp_diags: &LuaTable) -> LuaResult<Vec<RelatedInfo>> {
        let mut out = vec![];
        for lsp_diag in lsp_diags.sequence_values::<LuaTable>().flatten() {
            // Not all LSPs have "user_data.lsp.relatedInformation", skip those which does't
            let Ok(rel_infos) = lsp_diag
                .get::<LuaTable>("user_data")
                .and_then(|x| x.get::<LuaTable>("lsp"))
                .and_then(|x| x.get::<LuaTable>("relatedInformation"))
            else {
                continue;
            };

            for table in rel_infos.sequence_values::<LuaTable>().flatten() {
                // All LSPs "user_data.lsp.relatedInformation" should be deserializable into [`RelatedInfo`]
                out.push(RelatedInfo::from_related_info(&table)?);
            }
        }
        Ok(out)
    }
}

impl DiagnosticsFilter for RelatedInfoFilter {
    fn skip_diagnostic(&self, _buf_path: &str, lsp_diag: Option<&LuaTable>) -> LuaResult<bool> {
        let Some(lsp_diag) = lsp_diag else {
            return Ok(false);
        };
        if self.rel_infos.is_empty() {
            return Ok(false);
        }
        // All LSPs diagnostics should be deserializable into [`RelatedInfo`]
        let rel_info = RelatedInfo::from_lsp_diagnostic(lsp_diag)?;
        if self.rel_infos.contains(&rel_info) {
            return Ok(true);
        }
        Ok(false)
    }
}

/// Common shape of a root LSP diagnostic and the elements of its "user_data.lsp.relatedInformation".
#[derive(PartialEq)]
struct RelatedInfo {
    message: String,
    lnum: usize,
    col: usize,
    end_lnum: usize,
    end_col: usize,
}

impl RelatedInfo {
    /// Create a [`RelatedInfo`] from a root LSP diagnostic.
    fn from_lsp_diagnostic(table: &LuaTable) -> LuaResult<Self> {
        Ok(Self {
            message: table.get("message")?,
            lnum: table.get("lnum")?,
            col: table.get("col")?,
            end_lnum: table.get("end_lnum")?,
            end_col: table.get("end_col")?,
        })
    }

    /// Create a [`RelatedInfo`] from an element of an LSP diagnostic "user_data.lsp.relatedInformation" section.
    fn from_related_info(table: &LuaTable) -> LuaResult<Self> {
        let (start, end) = {
            let range = table
                .get::<LuaTable>("location")
                .and_then(|x| x.get::<LuaTable>("range"))?;
            (
                range.get::<LuaTable>("start")?,
                range.get::<LuaTable>("end")?,
            )
        };

        Ok(Self {
            message: table.get("message")?,
            lnum: start.get("line")?,
            col: start.get("character")?,
            end_lnum: end.get("line")?,
            end_col: end.get("character")?,
        })
    }
}
