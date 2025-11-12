//! Collection and construction of diagnostic filters.
//!
//! Defines [`DiagnosticsFilter`] trait plus ordered creation of all active filters (message blacklist,
//! source‑specific sets, related info deduper). Ordering is significant for short‑circuit behavior.

use color_eyre::eyre::bail;
use nvim_oxi::Dictionary;
use nvim_oxi::api::Buffer;
use ytil_nvim_oxi::buffer::BufferExt;
use ytil_nvim_oxi::dict::DictionaryExt as _;

use crate::diagnostics::filters::lsps::harper_ls::HarperLsFilter;
use crate::diagnostics::filters::lsps::typos_lsp::TyposLspFilter;
use crate::diagnostics::filters::related_info::RelatedInfoFilter;

pub mod buffer;
pub mod lsps;
pub mod related_info;

/// Represents a buffer associated with its filepath.
pub struct BufferWithPath {
    /// The buffer instance.
    buffer: Box<dyn BufferExt>,
    /// The filepath associated with the buffer.
    path: String,
}

impl TryFrom<Buffer> for BufferWithPath {
    type Error = color_eyre::eyre::Error;

    fn try_from(value: Buffer) -> Result<Self, Self::Error> {
        let path = value.get_name().map(|s| s.to_string_lossy().to_string())?;
        Ok(Self {
            path,
            buffer: Box::new(value),
        })
    }
}

/// Represents the location of a diagnostic in a file.
#[derive(Debug)]
#[cfg_attr(test, derive(Eq, PartialEq))]
struct DiagnosticLocation {
    /// The 1-based line number where the diagnostic starts.
    lnum: usize,
    /// The 0-based column number where the diagnostic starts.
    col: usize,
    /// The 0-based column number where the diagnostic ends.
    end_col: usize,
    /// The 1-based line number where the diagnostic ends.
    end_lnum: usize,
}

impl DiagnosticLocation {
    /// Returns the start position of the diagnostic as (line, column).
    ///
    /// # Returns
    /// A tuple containing the 1-based line number and 0-based column number.
    pub const fn start(&self) -> (usize, usize) {
        (self.lnum, self.col)
    }

    /// Returns the end position of the diagnostic as (line, column).
    ///
    /// # Returns
    /// A tuple containing the 1-based line number and 0-based column number.
    pub const fn end(&self) -> (usize, usize) {
        (self.end_lnum, self.end_col)
    }
}

impl TryFrom<&Dictionary> for DiagnosticLocation {
    type Error = color_eyre::eyre::Error;

    /// Attempts to convert a Nvim dictionary into a `DiagnosticLocation`.
    ///
    /// # Arguments
    /// - `value` A reference to a [`Dictionary`] containing diagnostic location fields.
    ///
    /// # Returns
    /// - `Ok(DiagnosticLocation)` if conversion succeeds and boundaries are consistent.
    ///
    /// # Errors
    /// - If required fields (`lnum`, `col`, `end_col`, `end_lnum`) are missing or invalid.
    /// - If integer conversion to `usize` fails.
    /// - If start position is after end position (inconsistent boundaries).
    fn try_from(value: &Dictionary) -> Result<Self, Self::Error> {
        let lnum = value
            .get_t::<nvim_oxi::Integer>("lnum")
            .and_then(|n| usize::try_from(n).map_err(From::from))?;
        let col = value
            .get_t::<nvim_oxi::Integer>("col")
            .and_then(|n| usize::try_from(n).map_err(From::from))?;
        let end_col = value
            .get_t::<nvim_oxi::Integer>("end_col")
            .and_then(|n| usize::try_from(n).map_err(From::from))?;
        let end_lnum = value
            .get_t::<nvim_oxi::Integer>("end_lnum")
            .and_then(|n| usize::try_from(n).map_err(From::from))?;

        if lnum > end_lnum {
            bail!("inconsistent line boundaries lnum {lnum} > end_lnum {end_lnum}");
        }
        if col > end_col {
            bail!("inconsistent col boundaries col {col} > end_col {end_col}");
        }

        Ok(Self {
            lnum,
            col,
            end_col,
            end_lnum,
        })
    }
}

/// Trait for filtering diagnostics.
pub trait DiagnosticsFilter {
    /// Returns true if the diagnostic should be skipped.
    ///
    /// # Arguments
    /// - `buf`: Buffer with path information.
    /// - `lsp_diag`: LSP diagnostic dictionary.
    ///
    /// # Errors
    /// - Access to required diagnostic fields (dictionary keys) fails (missing key or wrong type).
    /// - Filter-specific logic (e.g. related info extraction) fails.
    fn skip_diagnostic(&self, buf: &BufferWithPath, lsp_diag: &Dictionary) -> color_eyre::Result<bool>;
}

/// A collection of diagnostic filters.
pub struct DiagnosticsFilters(Vec<Box<dyn DiagnosticsFilter>>);

impl DiagnosticsFilters {
    /// Creates all available diagnostic filters. The order of filters is IMPORTANT.
    ///
    /// # Errors
    /// - Constructing the related info filter fails (dictionary traversal or type mismatch).
    pub fn all(lsp_diags: &[Dictionary]) -> color_eyre::Result<Self> {
        let mut filters = TyposLspFilter::filters();
        filters.extend(HarperLsFilter::filters());
        filters.push(Box::new(RelatedInfoFilter::new(lsp_diags)?));
        Ok(Self(filters))
    }
}

/// Implementation of [`DiagnosticsFilter`] for [`DiagnosticsFilters`].
impl DiagnosticsFilter for DiagnosticsFilters {
    /// Returns true if any filter skips the diagnostic.
    ///
    /// # Errors
    /// - A filter implementation (invoked in sequence) returns an error; it is propagated unchanged.
    fn skip_diagnostic(&self, buf: &BufferWithPath, lsp_diag: &Dictionary) -> color_eyre::Result<bool> {
        // The first filter that returns true skips the LSP diagnostic and all subsequent filters
        // evaluation.
        for filter in &self.0 {
            if filter.skip_diagnostic(buf, lsp_diag)? {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn try_from_valid_dictionary_succeeds() {
        let dict = create_diag(0, 1, 2, 3);
        assert2::let_assert!(Ok(loc) = DiagnosticLocation::try_from(&dict));
        pretty_assertions::assert_eq!(loc.lnum, 0);
        pretty_assertions::assert_eq!(loc.col, 1);
        pretty_assertions::assert_eq!(loc.end_lnum, 2);
        pretty_assertions::assert_eq!(loc.end_col, 3);
    }

    #[test]
    fn try_from_missing_lnum_key_fails() {
        let dict = ytil_nvim_oxi::dict! { col: 1_i64, end_col: 3_i64, end_lnum: 2_i64 };
        assert2::let_assert!(Err(err) = DiagnosticLocation::try_from(&dict));
        assert!(err.to_string().contains("missing dict value"));
    }

    #[test]
    fn try_from_wrong_type_for_lnum_fails() {
        let dict = ytil_nvim_oxi::dict! { lnum: "not_an_int", col: 1_i64, end_col: 3_i64, end_lnum: 2_i64 };
        assert2::let_assert!(Err(err) = DiagnosticLocation::try_from(&dict));
        assert!(err.to_string().contains(r#"value "not_an_int" of key "lnum""#));
        assert!(err.to_string().contains("is String but Integer was expected"));
    }

    #[test]
    fn try_from_negative_lnum_fails() {
        let dict = create_diag(-1, 1, 2, 3);
        assert2::let_assert!(Err(err) = DiagnosticLocation::try_from(&dict));
        assert!(err.to_string().contains("out of range"));
    }

    #[test]
    fn try_from_lnum_greater_than_end_lnum_fails() {
        let dict = create_diag(2, 1, 0, 3);
        assert2::let_assert!(Err(err) = DiagnosticLocation::try_from(&dict));
        assert!(err.to_string().contains("inconsistent line boundaries"));
        assert!(err.to_string().contains("lnum 2 > end_lnum 0"));
    }

    #[test]
    fn try_from_col_greater_than_end_col_fails() {
        let dict = create_diag(0, 3, 2, 1);
        assert2::let_assert!(Err(err) = DiagnosticLocation::try_from(&dict));
        assert!(err.to_string().contains("inconsistent col boundaries"));
        assert!(err.to_string().contains("col 3 > end_col 1"));
    }

    #[test]
    fn try_from_equal_boundaries_succeeds() {
        let dict = create_diag(1, 2, 1, 2);
        assert2::let_assert!(Ok(loc) = DiagnosticLocation::try_from(&dict));
        pretty_assertions::assert_eq!(
            loc,
            DiagnosticLocation {
                lnum: 1,
                col: 2,
                end_lnum: 1,
                end_col: 2
            }
        );
    }

    fn create_diag(lnum: i64, col: i64, end_lnum: i64, end_col: i64) -> Dictionary {
        ytil_nvim_oxi::dict! { col: col, end_col: end_col, lnum: lnum, end_lnum: end_lnum }
    }
}
