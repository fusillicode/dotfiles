use mlua::prelude::*;

/// Filters out the LSP diagnostics that are already represented by other ones.
/// E.g. HINTs pointing to a location already mentioned by other ERROR's rendered message.
///
/// The function uses a [`LuaTable`] rather than a user defined type that implements the [`FromLua`]
/// trait because the deserialization logic in this case is incremental.
/// I don't the complete "user_data.lsp.relatedInformation" upstream to handle the filtering.
pub fn filter_diagnostics(
    lua: &Lua,
    (buf_path, lsp_diags): (LuaString, LuaTable),
) -> LuaResult<LuaTable> {
    let mut out = vec![];

    let buf_path = buf_path.to_string_lossy();
    if unwanted_paths().iter().any(|up| buf_path.contains(up)) {
        return lua.create_sequence_from(out);
    }

    let rel_infos = get_related_infos(&lsp_diags)?;
    if rel_infos.is_empty() {
        return Ok(lsp_diags);
    }

    for table in lsp_diags.sequence_values::<LuaTable>().flatten() {
        // All LSPs diagnostics should be deserializable into [`RelatedInfo`]
        let rel_info = RelatedInfo::from_lsp_diagnostic(&table)?;
        if !rel_infos.contains(&rel_info) {
            out.push(table);
        }
    }

    lua.create_sequence_from(out)
}

// List of paths for which I don't want to report any diagnostic.
fn unwanted_paths() -> Vec<String> {
    let home_path = std::env::var("HOME");
    [home_path.map(|x| format!("{x}/.cargo")).ok()]
        .into_iter()
        .flatten()
        .collect()
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
