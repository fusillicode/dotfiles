//! Message blacklist configuration for the `typos` LSP source.
//!
//! Provides a curated set of substrings to suppress recurring false‑positive spelling suggestions
//! (domain‑specific terms) via [`MsgBlacklistFilter`].

use std::collections::HashSet;

use ytil_nvim_oxi::dict::DictionaryExt as _;

use crate::diagnostics::filters::BufferWithPath;
use crate::diagnostics::filters::DiagnosticsFilter;

pub struct TyposLspFilter<'a> {
    /// LSP diagnostic source name; only diagnostics from this source are eligible for blacklist matching.
    pub source: &'a str,
    /// Blacklist of messages per source.
    pub blacklist: HashSet<&'static str>,
    /// Optional buffer path substring that must be contained within the buffer path for filtering to apply.
    pub buf_path: Option<&'a str>,
}

impl TyposLspFilter<'_> {
    /// Build typos LSP diagnostic filters.
    ///
    /// Returns a vector of boxed [`DiagnosticsFilter`] configured for the typos
    /// language server. Includes a single [`MsgBlacklistFilter`] that suppresses
    /// false-positive spelling suggestions matching predefined substrings.
    ///
    /// # Returns
    /// - [`Vec<Box<dyn DiagnosticsFilter>>`] Collection containing one configured [`MsgBlacklistFilter`] for the typos
    ///   source.
    pub fn filters() -> Vec<Box<dyn DiagnosticsFilter>> {
        let blacklist = HashSet::from([
            "accidentes",
            "aci",
            "administrar",
            "anual",
            "aplicable",
            "autor",
            "calle",
            "clase",
            "clea",
            "cliente",
            "clientes",
            "comercial",
            "conceptos",
            "confidencial",
            "constituye",
            "decisiones",
            "emision",
            "explosivas",
            "foto",
            "importante",
            "individuales",
            "informativo",
            "informe",
            "internacional",
            "legislativo",
            "limite",
            "materiales",
            "materias",
            "minerales",
            "momento",
            "nd",
            "ot",
            "patrones",
            "presentes",
            "producto",
            "profesional",
            "regulatorias",
            "responsable",
            "ser",
            "ue",
            "utiliza",
        ]);

        vec![Box::new(TyposLspFilter {
            source: "typos",
            buf_path: None,
            blacklist,
        })]
    }
}

impl DiagnosticsFilter for TyposLspFilter<'_> {
    fn skip_diagnostic(
        &self,
        buf: Option<&BufferWithPath>,
        lsp_diag: Option<&nvim_oxi::Dictionary>,
    ) -> color_eyre::Result<bool> {
        let (Some(buf), Some(lsp_diag)) = (buf, lsp_diag) else {
            return Ok(false);
        };
        if let Some(ref bp) = self.buf_path
            && !buf.path.contains(bp)
        {
            return Ok(false);
        }
        if let Some(source) = lsp_diag.get_opt_t::<nvim_oxi::String>("source")?
            && self.source != source
        {
            return Ok(false);
        }
        let msg = lsp_diag.get_t::<nvim_oxi::String>("message")?;

        Ok(self
            .blacklist
            .iter()
            .any(|blacklisted_msg| msg.contains(blacklisted_msg)))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use crate::diagnostics::filters::BufferWithPath;

    fn create_buffer_with_path(path: &str) -> BufferWithPath {
        BufferWithPath {
            buffer: Box::new(ytil_nvim_oxi::buffer::mock::MockBuffer(vec![])),
            path: path.to_string(),
        }
    }

    #[test]
    fn skip_diagnostic_when_no_diagnostic_returns_false() {
        let filter = TyposLspFilter {
            source: "typos",
            blacklist: HashSet::from(["test"]),
            buf_path: None,
        };
        assert2::let_assert!(Ok(res) = filter.skip_diagnostic(None, None));
        assert!(!res);
    }

    #[test]
    fn skip_diagnostic_when_buf_path_pattern_not_matched_returns_false() {
        let filter = TyposLspFilter {
            source: "typos",
            blacklist: HashSet::from(["test"]),
            buf_path: Some("src/"),
        };
        let buf = create_buffer_with_path("tests/main.rs");
        let diag = dict! {
            source: "typos",
            message: "some test message",
        };
        assert2::let_assert!(Ok(res) = filter.skip_diagnostic(Some(&buf), Some(&diag)));
        assert!(!res);
    }

    #[test]
    fn skip_diagnostic_when_source_mismatch_returns_false() {
        let filter = TyposLspFilter {
            source: "typos",
            blacklist: HashSet::from(["test"]),
            buf_path: None,
        };
        let buf = create_buffer_with_path("src/lib.rs");
        let diag = dict! {
            source: "other",
            message: "some test message",
        };
        assert2::let_assert!(Ok(res) = filter.skip_diagnostic(Some(&buf), Some(&diag)));
        assert!(!res);
    }

    #[test]
    fn skip_diagnostic_when_message_does_not_contain_blacklisted_substring_returns_false() {
        let filter = TyposLspFilter {
            source: "typos",
            blacklist: HashSet::from(["test"]),
            buf_path: None,
        };
        let buf = create_buffer_with_path("src/lib.rs");
        let diag = dict! {
            source: "typos",
            message: "some other message",
        };
        assert2::let_assert!(Ok(res) = filter.skip_diagnostic(Some(&buf), Some(&diag)));
        assert!(!res);
    }

    #[test]
    fn skip_diagnostic_when_message_contains_blacklisted_substring_returns_true() {
        let filter = TyposLspFilter {
            source: "typos",
            blacklist: HashSet::from(["test"]),
            buf_path: None,
        };
        let buf = create_buffer_with_path("src/lib.rs");
        let diag = dict! {
            source: "typos",
            message: "some test message",
        };
        assert2::let_assert!(Ok(res) = filter.skip_diagnostic(Some(&buf), Some(&diag)));
        assert!(res);
    }

    #[test]
    fn skip_diagnostic_when_missing_message_key_returns_error() {
        let filter = TyposLspFilter {
            source: "typos",
            blacklist: HashSet::from(["test"]),
            buf_path: None,
        };
        let buf = create_buffer_with_path("src/lib.rs");
        let diag = dict! {
            source: "typos",
        };
        assert2::let_assert!(Err(err) = filter.skip_diagnostic(Some(&buf), Some(&diag)));
        pretty_assertions::assert_eq!(
            err.to_string(),
            "missing dict value | query=[\n    \"message\",\n] dict={ source: \"typos\" }"
        );
    }
}
