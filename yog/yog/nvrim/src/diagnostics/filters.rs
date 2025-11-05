//! Collection and construction of diagnostic filters.
//!
//! Defines [`DiagnosticsFilter`] trait plus ordered creation of all active filters (message blacklist,
//! source‑specific sets, related info deduper). Ordering is significant for short‑circuit behavior.

use color_eyre::eyre::bail;
use nvim_oxi::Dictionary;
use nvim_oxi::api::Buffer;
use nvim_oxi::api::opts::GetTextOpts;
use ytil_nvim_oxi::buffer::BufferExt;
use ytil_nvim_oxi::dict::DictionaryExt as _;

use crate::diagnostics::filters::lsps::harper_ls::HarperLsFilter;
use crate::diagnostics::filters::lsps::typos_lsp::TyposLspFilter;
use crate::diagnostics::filters::related_info::RelatedInfoFilter;

pub mod buffer;
pub mod lsps;
pub mod related_info;

pub struct BufferWithPath {
    buffer: Box<dyn BufferExt>,
    path: String,
}

impl BufferWithPath {
    pub fn get_diagnosed_text(&self, lsp_diag: &Dictionary) -> color_eyre::Result<Option<String>> {
        let Some(loc) = DiagnosticLocation::try_from(lsp_diag).ok() else {
            return Ok(None);
        };

        let lines = self
            .buffer
            .get_text_between(loc.start(), loc.end(), &GetTextOpts::default())?;

        let lines_len = lines.len();
        if lines_len == 0 {
            return Ok(None);
        }
        let last_line_idx = lines_len.saturating_sub(1);
        let adjusted_end_col = loc.adjusted_end_col();

        let mut out = String::new();
        for (line_idx, line) in lines.iter().enumerate() {
            let line = line.clone();
            let text = if line_idx == last_line_idx {
                line.get(..adjusted_end_col).unwrap_or(&line)
            } else {
                &line
            };
            out.push_str(text);
        }

        if out.is_empty() {
            return Ok(None);
        }

        Ok(Some(out))
    }

    pub fn path(&self) -> &str {
        &self.path
    }
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

#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
struct DiagnosticLocation {
    lnum: usize,
    col: usize,
    end_col: usize,
    end_lnum: usize,
}

impl DiagnosticLocation {
    pub const fn start(&self) -> (usize, usize) {
        (self.lnum, self.col)
    }

    pub const fn end(&self) -> (usize, usize) {
        (self.end_lnum, self.end_col)
    }

    pub const fn adjusted_end_col(&self) -> usize {
        self.end_col.saturating_sub(self.col)
    }
}

impl TryFrom<&Dictionary> for DiagnosticLocation {
    type Error = color_eyre::eyre::Error;

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
            bail!("inconsistent boundaries {}", stringify!(lnum > end_lnum));
        }
        if col > end_col {
            bail!("inconsistent boundaries {}", stringify!(col > end_col));
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
    use rstest::rstest;
    use ytil_nvim_oxi::buffer::mock::MockBuffer;

    use super::*;

    #[rstest]
    #[case::lnum_greater_than_end_lnum(
        vec!["hello world".to_string()],
        create_diag(1, 0, 0, 5),
        None
    )]
    #[case::col_greater_than_end_col(
        vec!["hello world".to_string()],
        create_diag(0, 5, 0, 0),
        None
    )]
    #[case::empty_lines(
        vec![],
        create_diag(0, 0, 0, 5),
        None
    )]
    #[case::single_line_partial_word(
        vec!["hello world".to_string()],
        create_diag(0, 0, 0, 5),
        Some("hello".to_string())
    )]
    #[case::single_line_full_word(
        vec!["hello".to_string()],
        create_diag(0, 0, 0, 5),
        Some("hello".to_string())
    )]
    #[case::multi_line_word(
        vec!["heal".to_string(), "lo".to_string()],
        create_diag(0, 0, 1, 2),
        Some("heallo".to_string())
    )]
    #[case::multi_line_partial_last_line(
        vec!["heal".to_string(), "lo world".to_string()],
        create_diag(0, 0, 1, 2),
        Some("heallo".to_string())
    )]
    #[case::start_col_out_of_bounds(
        vec!["hi".to_string()],
        create_diag(0, 10, 0, 15),
        None
    )]
    #[case::end_col_beyond_line(
        vec!["hi".to_string()],
        create_diag(0, 0, 0, 10),
        Some("hi".to_string())
    )]
    fn get_diagnosed_text_returns_expected_text(
        #[case] lines: Vec<String>,
        #[case] diag: Dictionary,
        #[case] expected: Option<String>,
    ) {
        let buf = BufferWithPath {
            buffer: Box::new(MockBuffer(lines)),
            path: "test.rs".to_string(),
        };
        assert2::let_assert!(Ok(actual) = buf.get_diagnosed_text(&diag));
        pretty_assertions::assert_eq!(actual, expected);
    }

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
        assert!(err.to_string().contains("inconsistent boundaries"));
        assert!(err.to_string().contains("lnum > end_lnum"));
    }

    #[test]
    fn try_from_col_greater_than_end_col_fails() {
        let dict = create_diag(0, 3, 2, 1);
        assert2::let_assert!(Err(err) = DiagnosticLocation::try_from(&dict));
        assert!(err.to_string().contains("inconsistent boundaries"));
        assert!(err.to_string().contains("col > end_col"));
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
