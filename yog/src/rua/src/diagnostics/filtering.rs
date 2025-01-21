use mlua::prelude::*;

use crate::utils::dig;

/// Filters out the LSP diagnostics that are already represented by other ones, e.g. HINTs pointing
/// to location already mentioned by other ERROR's rendered message.
pub fn filter_diagnostics(lua: &Lua, lsp_diags: LuaTable) -> LuaResult<LuaTable> {
    let rel_info_diags = get_related_info_diag(&lsp_diags)?;
    if rel_info_diags.is_empty() {
        return Ok(lsp_diags);
    }
    let mut out = vec![];
    for lsp_diag in lsp_diags.sequence_values::<LuaTable>().flatten() {
        let rel = RelatedInfoDiag {
            msg: dig::<String>(&lsp_diag, &["message"])?,
            start: Pos {
                ln: dig::<usize>(&lsp_diag, &["lnum"])?,
                col: dig::<usize>(&lsp_diag, &["col"])?,
            },
            end: Pos {
                ln: dig::<usize>(&lsp_diag, &["end_lnum"])?,
                col: dig::<usize>(&lsp_diag, &["end_col"])?,
            },
        };
        if !rel_info_diags.contains(&rel) {
            out.push(lsp_diag);
        }
    }
    lua.create_sequence_from(out)
}

/// Get the message and posisiton of the LSP "relatedInformation"s inside LSP diagnostics.
fn get_related_info_diag(lsp_diags: &LuaTable) -> LuaResult<Vec<RelatedInfoDiag>> {
    let mut rel_diags = vec![];
    for lsp_diag in lsp_diags.sequence_values::<LuaTable>().flatten() {
        let Ok(rel_infos) = dig::<LuaTable>(&lsp_diag, &["user_data", "lsp", "relatedInformation"])
        else {
            continue;
        };
        for rel_info in rel_infos.sequence_values::<LuaTable>().flatten() {
            let start = dig::<LuaTable>(&rel_info, &["location", "range", "start"])?;
            let end = dig::<LuaTable>(&rel_info, &["location", "range", "end"])?;
            rel_diags.push(RelatedInfoDiag {
                msg: dig::<String>(&rel_info, &["message"])?,
                start: Pos {
                    ln: dig::<usize>(&start, &["line"])?,
                    col: dig::<usize>(&start, &["character"])?,
                },
                end: Pos {
                    ln: dig::<usize>(&end, &["line"])?,
                    col: dig::<usize>(&end, &["character"])?,
                },
            });
        }
    }
    Ok(rel_diags)
}

#[derive(Debug, PartialEq, Eq, Clone)]
struct RelatedInfoDiag {
    msg: String,
    start: Pos,
    end: Pos,
}

#[derive(Debug, PartialEq, Eq, Clone)]
struct Pos {
    ln: usize,
    col: usize,
}
