use nvim_oxi::Dictionary;

use crate::DictionaryExt;
use crate::diagnostics::filters::DiagnosticsFilter;

/// Filters out diagnostics already represented by other ones
/// (e.g. HINTs pointing to a location already mentioned by other ERROR's rendered message)
pub struct RelatedInfoFilter {
    rel_infos: Vec<RelatedInfo>,
}

impl RelatedInfoFilter {
    pub fn new(lsp_diags: &[&Dictionary]) -> color_eyre::Result<Self> {
        Ok(Self {
            rel_infos: Self::get_related_infos(lsp_diags)?,
        })
    }

    /// Get the [RelatedInfo]s of an LSP diagnostic represented by a [Dictionary].
    fn get_related_infos(lsp_diags: &[&Dictionary]) -> color_eyre::Result<Vec<RelatedInfo>> {
        let mut out = vec![];
        for lsp_diag in lsp_diags {
            // Not all LSPs have "user_data.lsp.relatedInformation", skip those which doesn't
            let Some(rel_infos) = lsp_diag.get_dict(&["user_data", "lsp", "relatedInformation"])? else {
                continue;
            };

            for table in rel_infos.as_slice() {
                // All LSPs "user_data.lsp.relatedInformation" should be deserializable into [RelatedInfo]
                let dict = Dictionary::try_from(table.clone().into_value())?;
                out.push(RelatedInfo::from_related_info(&dict)?);
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
    lnum: usize,
    col: usize,
    end_lnum: usize,
    end_col: usize,
}

impl RelatedInfo {
    /// Create a [RelatedInfo] from a root LSP diagnostic.
    fn from_lsp_diagnostic(dict: &Dictionary) -> color_eyre::Result<Self> {
        Ok(Self {
            message: dict.get_string("message")?.unwrap(),
            lnum: dict.get_string("lnum")?.unwrap().parse()?,
            col: dict.get_string("col")?.unwrap().parse()?,
            end_lnum: dict.get_string("end_lnum")?.unwrap().parse()?,
            end_col: dict.get_string("end_col")?.unwrap().parse()?,
        })
    }

    /// Create a [RelatedInfo] from an element of an LSP diagnostic "user_data.lsp.relatedInformation" section.
    fn from_related_info(dict: &Dictionary) -> color_eyre::Result<Self> {
        let (start, end) = {
            let range = dict.get_dict(&["location", "range"])?.unwrap();
            (range.get_dict(&["start"])?.unwrap(), range.get_dict(&["end"])?.unwrap())
        };

        Ok(Self {
            message: dict.get_string("message")?.unwrap(),
            lnum: start.get_string("line")?.unwrap().parse()?,
            col: start.get_string("character")?.unwrap().parse()?,
            end_lnum: end.get_string("line")?.unwrap().parse()?,
            end_col: end.get_string("character")?.unwrap().parse()?,
        })
    }
}
