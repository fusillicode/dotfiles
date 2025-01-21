use mlua::prelude::*;

/// Filters out the LSP diagnostics that are already represented by other ones.
/// E.g. HINTs pointing to location already mentioned by other ERROR's rendered message.
pub fn filter_diagnostics(lua: &Lua, lsp_diags: LuaTable) -> LuaResult<LuaTable> {
    let rel_infos = get_related_infos(&lsp_diags)?;
    if rel_infos.is_empty() {
        return Ok(lsp_diags);
    }

    let mut out = vec![];
    for table in lsp_diags.sequence_values::<LuaTable>().flatten() {
        // All LSPs diagnostics should be deserializable into [`RelatedInfo`]
        let rel_info = RelatedInfo::from_root(&table)?;
        if !rel_infos.contains(&rel_info) {
            out.push(table);
        }
    }

    lua.create_sequence_from(out)
}

/// Get the message and posisiton of the LSP "relatedInformation" inside LSP diagnostics.
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

#[derive(PartialEq)]
struct RelatedInfo {
    message: String,
    lnum: usize,
    col: usize,
    end_lnum: usize,
    end_col: usize,
}

impl RelatedInfo {
    fn from_root(table: &LuaTable) -> LuaResult<Self> {
        Ok(Self {
            message: table.get("message")?,
            lnum: table.get("lnum")?,
            col: table.get("col")?,
            end_lnum: table.get("end_lnum")?,
            end_col: table.get("end_col")?,
        })
    }

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
