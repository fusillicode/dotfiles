//! "typos-lsp" custom filter.
//!
//! Suppresses noisy diagnostics that cannot be filtered directly with "typos-lsp".

use std::collections::HashSet;
use std::sync::LazyLock;

use lit2::set;

use crate::diagnostics::filters::BufferWithPath;
use crate::diagnostics::filters::DiagnosticsFilter;
use crate::diagnostics::filters::lsps::GetDiagMsgOutput;
use crate::diagnostics::filters::lsps::LspFilter;

/// Static blacklist initialized once on first access.
/// Contains false-positive spelling suggestions to suppress.
static TYPOS_BLACKLIST: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    set![
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
        "sorce",
        "ue",
        "utiliza",
    ]
});

pub struct TyposLspFilter<'a> {
    /// LSP diagnostic source name; only diagnostics from this source are eligible for blacklist matching.
    pub source: &'a str,
    /// Blacklist of messages per source. References the static blacklist for one-time initialization.
    pub blacklist: &'a HashSet<&'static str>,
    /// Optional buffer path substring that must be contained within the buffer path for filtering to apply.
    pub path_substring: Option<&'a str>,
}

impl TyposLspFilter<'_> {
    /// Build typos LSP diagnostic filters.
    ///
    /// Returns a vector of boxed [`DiagnosticsFilter`] configured for the typos
    /// language server. Includes a single [`TyposLspFilter`] that suppresses
    /// false-positive spelling suggestions matching predefined substrings.
    pub fn filters() -> Vec<Box<dyn DiagnosticsFilter>> {
        vec![Box::new(TyposLspFilter {
            source: "typos",
            path_substring: None,
            blacklist: &TYPOS_BLACKLIST,
        })]
    }
}

impl LspFilter for TyposLspFilter<'_> {
    fn path_substring(&self) -> Option<&str> {
        self.path_substring
    }

    fn source(&self) -> &str {
        self.source
    }
}

impl DiagnosticsFilter for TyposLspFilter<'_> {
    fn skip_diagnostic(&self, buf: &BufferWithPath, lsp_diag: &nvim_oxi::Dictionary) -> rootcause::Result<bool> {
        let diag_msg = match self.get_diag_msg_or_skip(&buf.path, lsp_diag)? {
            GetDiagMsgOutput::Msg(diag_msg) => diag_msg,
            GetDiagMsgOutput::Skip => return Ok(false),
        };

        Ok(self
            .blacklist
            .iter()
            .any(|blacklisted_msg| diag_msg.contains(blacklisted_msg)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::filters::BufferWithPath;

    fn create_buffer_with_path(path: &str) -> BufferWithPath {
        BufferWithPath {
            buffer: Box::new(ytil_noxi::buffer::mock::MockBuffer::new(vec![])),
            path: path.to_string(),
        }
    }

    #[test]
    fn skip_diagnostic_when_buf_path_pattern_not_matched_returns_false() {
        let test_blacklist = set!["test"];
        let filter = TyposLspFilter {
            source: "typos",
            blacklist: &test_blacklist,
            path_substring: Some("src/"),
        };
        let buf = create_buffer_with_path("tests/main.rs");
        let diag = dict! {
            source: "typos",
            message: "some test message",
        };
        assert2::let_assert!(Ok(res) = filter.skip_diagnostic(&buf, &diag));
        assert!(!res);
    }

    #[test]
    fn skip_diagnostic_when_source_mismatch_returns_false() {
        let test_blacklist = set!["test"];
        let filter = TyposLspFilter {
            source: "typos",
            blacklist: &test_blacklist,
            path_substring: None,
        };
        let buf = create_buffer_with_path("src/lib.rs");
        let diag = dict! {
            source: "other",
            message: "some test message",
        };
        assert2::let_assert!(Ok(res) = filter.skip_diagnostic(&buf, &diag));
        assert!(!res);
    }

    #[test]
    fn skip_diagnostic_when_message_does_not_contain_blacklisted_substring_returns_false() {
        let test_blacklist = set!["test"];
        let filter = TyposLspFilter {
            source: "typos",
            blacklist: &test_blacklist,
            path_substring: None,
        };
        let buf = create_buffer_with_path("src/lib.rs");
        let diag = dict! {
            source: "typos",
            message: "some other message",
        };
        assert2::let_assert!(Ok(res) = filter.skip_diagnostic(&buf, &diag));
        assert!(!res);
    }

    #[test]
    fn skip_diagnostic_when_message_contains_blacklisted_substring_returns_true() {
        let test_blacklist = set!["test"];
        let filter = TyposLspFilter {
            source: "typos",
            blacklist: &test_blacklist,
            path_substring: None,
        };
        let buf = create_buffer_with_path("src/lib.rs");
        let diag = dict! {
            source: "typos",
            message: "some test message",
        };
        assert2::let_assert!(Ok(res) = filter.skip_diagnostic(&buf, &diag));
        assert!(res);
    }

    #[test]
    fn skip_diagnostic_when_missing_message_key_returns_error() {
        let test_blacklist = set!["test"];
        let filter = TyposLspFilter {
            source: "typos",
            blacklist: &test_blacklist,
            path_substring: None,
        };
        let buf = create_buffer_with_path("src/lib.rs");
        let diag = dict! {
            source: "typos",
        };
        assert2::let_assert!(Err(err) = filter.skip_diagnostic(&buf, &diag));
        assert_eq!(err.format_current_context().to_string(), "missing dict value");
    }
}
