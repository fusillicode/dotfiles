//! Filter for deduplicating diagnostics based on related information arrays.
//!
//! Extracts `user_data.lsp.relatedInformation` entries and skips root diagnostics whose rendered
//! information is already represented, reducing noise (especially repeated hints).

use std::collections::HashSet;

use color_eyre::eyre::Context;
use nvim_oxi::Array;
use nvim_oxi::Dictionary;
use nvim_oxi::ObjectKind;
use nvim_oxi::conversion::FromObject;
use ytil_noxi::dict::DictionaryExt;

use crate::diagnostics::filters::BufferWithPath;
use crate::diagnostics::filters::DiagnosticsFilter;

/// Filters out diagnostics already represented by other ones
/// (e.g. HINTs pointing to a location already mentioned by other ERROR's rendered message)
pub struct RelatedInfoFilter {
    /// The set of already-seen related infos extracted from LSP diagnostics.
    /// Used to skip duplicate root diagnostics that only repeat information.
    /// Uses [`HashSet`] for O(1) lookup instead of Vec's O(n).
    rel_infos: HashSet<RelatedInfo>,
}

impl RelatedInfoFilter {
    /// Creates a new [`RelatedInfoFilter`] from LSP diagnostics.
    ///
    /// # Errors
    /// - Extracting related information arrays fails (missing key or wrong type).
    pub fn new(lsp_diags: &[Dictionary]) -> color_eyre::Result<Self> {
        Ok(Self {
            rel_infos: Self::get_related_infos(lsp_diags)?,
        })
    }

    /// Get the [`RelatedInfo`] of an LSP diagnostic represented by a [`Dictionary`].
    ///
    /// # Errors
    /// - Traversing diagnostics fails (unexpected value kinds or conversion errors).
    fn get_related_infos(lsp_diags: &[Dictionary]) -> color_eyre::Result<HashSet<RelatedInfo>> {
        // Pre-allocate with estimated capacity (average ~2 related infos per diagnostic)
        let mut out = HashSet::with_capacity(lsp_diags.len().saturating_mul(2));
        for lsp_diag in lsp_diags {
            // Not all LSPs have "user_data.lsp.relatedInformation"; skip those missing it
            let Some(lsp) = lsp_diag.get_dict(&["user_data", "lsp"])? else {
                continue;
            };
            let rel_infos_key = "relatedInformation";
            let Some(rel_infos) = lsp.get(rel_infos_key) else {
                continue;
            };

            let rel_infos = Array::from_object(rel_infos.clone()).with_context(|| {
                ytil_noxi::extract::unexpected_kind_error_msg(rel_infos, rel_infos_key, &lsp, ObjectKind::Array)
            })?;
            for rel_info in rel_infos {
                let rel_info = Dictionary::try_from(rel_info)?;
                out.insert(RelatedInfo::from_related_info(&rel_info)?);
            }
        }
        Ok(out)
    }
}

impl DiagnosticsFilter for RelatedInfoFilter {
    /// Returns true if the diagnostic is related information already covered.
    ///
    /// # Errors
    /// - Building the candidate related info shape from the diagnostic fails.
    fn skip_diagnostic(&self, _buf: &BufferWithPath, lsp_diag: &Dictionary) -> color_eyre::Result<bool> {
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
#[derive(Eq, Hash, PartialEq)]
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
    /// - Required keys (`message`, `lnum`, `col`, `end_lnum`, `end_col`) are missing or of unexpected type.
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
    /// - Required nested keys (range.start, range.end, message, line/character) are missing or wrong type.
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
