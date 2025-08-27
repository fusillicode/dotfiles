use color_eyre::eyre::Context;
use nvim_oxi::Array;
use nvim_oxi::Dictionary;
use nvim_oxi::ObjectKind;
use nvim_oxi::conversion::FromObject;

use crate::DictionaryExt;
use crate::diagnostics::filters::DiagnosticsFilter;

/// Filters out diagnostics already represented by other ones
/// (e.g. HINTs pointing to a location already mentioned by other ERROR's rendered message)
pub struct RelatedInfoFilter {
    rel_infos: Vec<RelatedInfo>,
}

impl RelatedInfoFilter {
    pub fn new(lsp_diags: &[Dictionary]) -> color_eyre::Result<Self> {
        Ok(Self {
            rel_infos: Self::get_related_infos(lsp_diags)?,
        })
    }

    /// Get the [RelatedInfo]s of an LSP diagnostic represented by a [Dictionary].
    fn get_related_infos(lsp_diags: &[Dictionary]) -> color_eyre::Result<Vec<RelatedInfo>> {
        let mut out = vec![];
        for lsp_diag in lsp_diags {
            // Not all LSPs have "user_data.lsp.relatedInformation", skip those which doesn't
            let Some(lsp) = lsp_diag.get_dict(&["user_data", "lsp"])? else {
                continue;
            };
            let rel_infos_key = "relatedInformation";
            let Some(rel_infos) = lsp.get(rel_infos_key) else {
                continue;
            };

            let rel_infos = Array::from_object(rel_infos.clone())
                .with_context(|| crate::unexpected_kind_error_msg(rel_infos, rel_infos_key, &lsp, ObjectKind::Array))?;
            for rel_info in rel_infos {
                let rel_info = Dictionary::try_from(rel_info)?;
                out.push(RelatedInfo::from_related_info(&rel_info)?);
            }
        }
        Ok(out)
    }
}

impl DiagnosticsFilter for RelatedInfoFilter {
    fn skip_diagnostic(&self, _buf_path: &str, lsp_diag: Option<&Dictionary>) -> color_eyre::Result<bool> {
        let Some(lsp_diag) = lsp_diag else {
            return Ok(false);
        };
        if self.rel_infos.is_empty() {
            return Ok(false);
        }
        // All LSPs diagnostics should be deserializable into [RelatedInfo]
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
    lnum: i64,
    col: i64,
    end_lnum: i64,
    end_col: i64,
}

impl RelatedInfo {
    /// Create a [RelatedInfo] from a root LSP diagnostic.
    fn from_lsp_diagnostic(lsp_diagnostic: &Dictionary) -> color_eyre::Result<Self> {
        Ok(Self {
            message: lsp_diagnostic.get_string("message")?,
            lnum: lsp_diagnostic.get_i64("lnum")?,
            col: lsp_diagnostic.get_i64("col")?,
            end_lnum: lsp_diagnostic.get_i64("end_lnum")?,
            end_col: lsp_diagnostic.get_i64("end_col")?,
        })
    }

    /// Create a [RelatedInfo] from an element of an LSP diagnostic "user_data.lsp.relatedInformation" section.
    fn from_related_info(rel_info: &Dictionary) -> color_eyre::Result<Self> {
        let (start, end) = {
            let range_query = ["location", "range"];
            let range = rel_info
                .get_dict(&range_query)?
                .ok_or_else(|| crate::no_value_matching(&range_query, rel_info))?;

            let start_query = ["start"];
            let end_query = ["end"];
            (
                range
                    .get_dict(&start_query)?
                    .ok_or_else(|| crate::no_value_matching(&start_query, &range))?,
                range
                    .get_dict(&end_query)?
                    .ok_or_else(|| crate::no_value_matching(&end_query, &range))?,
            )
        };

        Ok(Self {
            message: rel_info.get_string("message")?,
            lnum: start.get_i64("line")?,
            col: start.get_i64("character")?,
            end_lnum: end.get_i64("line")?,
            end_col: end.get_i64("character")?,
        })
    }
}
