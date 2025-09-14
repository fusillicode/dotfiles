use color_eyre::eyre::Context;
use nvim_oxi::Array;
use nvim_oxi::Dictionary;
use nvim_oxi::ObjectKind;
use nvim_oxi::conversion::FromObject;

use crate::diagnostics::filters::DiagnosticsFilter;
use crate::oxi_ext::dict::DictionaryExt;

/// Filters out diagnostics already represented by other ones
/// (e.g. HINTs pointing to a location already mentioned by other ERROR's rendered message)
pub struct RelatedInfoFilter {
    /// The set of already-seen related infos extracted from LSP diagnostics.
    /// Used to skip duplicate root diagnostics that only repeat information.
    rel_infos: Vec<RelatedInfo>,
}

impl RelatedInfoFilter {
    /// Creates a new [`RelatedInfoFilter`] from LSP diagnostics.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - An underlying operation fails.
    pub fn new(lsp_diags: &[Dictionary]) -> color_eyre::Result<Self> {
        Ok(Self {
            rel_infos: Self::get_related_infos(lsp_diags)?,
        })
    }

    /// Get the [`RelatedInfo`] of an LSP diagnostic represented by a [`Dictionary`].
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - An underlying operation fails.
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

            let rel_infos = Array::from_object(rel_infos.clone()).with_context(|| {
                crate::oxi_ext::extract::unexpected_kind_error_msg(rel_infos, rel_infos_key, &lsp, ObjectKind::Array)
            })?;
            for rel_info in rel_infos {
                let rel_info = Dictionary::try_from(rel_info)?;
                out.push(RelatedInfo::from_related_info(&rel_info)?);
            }
        }
        Ok(out)
    }
}

impl DiagnosticsFilter for RelatedInfoFilter {
    /// Returns true if the diagnostic is related information already covered.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - An underlying operation fails.
    fn skip_diagnostic(&self, _buf_path: &str, lsp_diag: Option<&Dictionary>) -> color_eyre::Result<bool> {
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

/// Common shape of a root LSP diagnostic and the elements of its "`user_data.lsp.relatedInformation`".
#[derive(PartialEq, Eq)]
struct RelatedInfo {
    /// The starting column number.
    col: i64,
    /// The ending column number.
    end_col: i64,
    /// The ending line number.
    end_lnum: i64,
    /// The starting line number.
    lnum: i64,
    /// The diagnostic message.
    message: String,
}

impl RelatedInfo {
    /// Create a [`RelatedInfo`] from a root LSP diagnostic.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - An underlying operation fails.
    fn from_lsp_diagnostic(lsp_diagnostic: &Dictionary) -> color_eyre::Result<Self> {
        Ok(Self {
            message: lsp_diagnostic.get_t::<nvim_oxi::String>("message")?,
            lnum: lsp_diagnostic.get_t::<nvim_oxi::Integer>("lnum")?,
            col: lsp_diagnostic.get_t::<nvim_oxi::Integer>("col")?,
            end_lnum: lsp_diagnostic.get_t::<nvim_oxi::Integer>("end_lnum")?,
            end_col: lsp_diagnostic.get_t::<nvim_oxi::Integer>("end_col")?,
        })
    }

    /// Create a [`RelatedInfo`] from an element of an LSP diagnostic "`user_data.lsp.relatedInformation`" section.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - An underlying operation fails.
    fn from_related_info(rel_info: &Dictionary) -> color_eyre::Result<Self> {
        let (start, end) = {
            let range_query = ["location", "range"];
            let range = rel_info.get_required_dict(&range_query)?;

            let start_query = ["start"];
            let end_query = ["end"];
            (
                range.get_required_dict(&start_query)?,
                range.get_required_dict(&end_query)?,
            )
        };

        Ok(Self {
            message: rel_info.get_t::<nvim_oxi::String>("message")?,
            lnum: start.get_t::<nvim_oxi::Integer>("line")?,
            col: start.get_t::<nvim_oxi::Integer>("character")?,
            end_lnum: end.get_t::<nvim_oxi::Integer>("line")?,
            end_col: end.get_t::<nvim_oxi::Integer>("character")?,
        })
    }
}
