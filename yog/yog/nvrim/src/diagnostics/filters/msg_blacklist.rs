use nvim_oxi::Dictionary;

use crate::diagnostics::filters::DiagnosticsFilter;
use crate::oxi_ext::dict::DictionaryExt;

/// Filters out diagnostics whose messages contain any blacklisted substrings.
///
/// Filters diagnostics whose lowercase message contains any of the configured blacklist
/// substrings, provided:
/// - A diagnostic is present.
/// - The optional buffer path pattern (if set) is contained in the buffer path.
/// - The diagnostic's `source` differs from [`MsgBlacklistFilter::source`].
///
/// This struct is configured with:
/// - [`MsgBlacklistFilter::source`]: LSP source name used for source-difference gating.
/// - [`MsgBlacklistFilter::blacklist`]: Case-insensitive substrings (stored as provided, matched against a lowercase
///   message).
/// - [`MsgBlacklistFilter::buf_path`]: Optional substring pattern that must appear within the buffer path for filtering
///   to apply.
///
/// See [`DiagnosticsFilter`] for trait integration.
///
/// # Rationale
/// Some language servers or tools emit noisy diagnostics (e.g. repeated I/O channel mentions).
/// Keeping them out improves signal without mutating upstream server configuration.
///
/// # Future Work
/// - Source gating currently requires the diagnostic `source` to equal `MsgBlacklistFilter::source`; blacklist only
///   applies to that single source. Future improvement: support configurable mode (inclusive vs exclusive) or multiple
///   sources.
/// - Support regex-based patterns for messages and buffer paths.
/// - Provide statistics on filtered counts for observability.
pub struct MsgBlacklistFilter<'a> {
    /// LSP diagnostic source name; only diagnostics from this source are eligible for blacklist matching.
    pub source: &'a str,
    /// Blacklist of messages per source.
    pub blacklist: Vec<String>,
    /// The buffer path pattern to match.
    pub buf_path: Option<&'a str>,
}

impl DiagnosticsFilter for MsgBlacklistFilter<'_> {
    /// Returns true if the diagnostic message is blacklisted.
    ///
    /// # Errors
    /// - Required `source` or `message` keys are missing or have unexpected types.
    fn skip_diagnostic(&self, buf_path: &str, lsp_diag: Option<&Dictionary>) -> color_eyre::Result<bool> {
        let Some(lsp_diag) = lsp_diag else {
            return Ok(false);
        };
        if let Some(ref bp) = self.buf_path
            && !buf_path.contains(bp)
        {
            return Ok(false);
        }
        if self.source != lsp_diag.get_t::<nvim_oxi::String>("source")? {
            return Ok(false);
        }
        let msg = lsp_diag.get_t::<nvim_oxi::String>("message")?.to_lowercase();
        if self.blacklist.iter().any(|b| msg.contains(b)) {
            return Ok(true);
        }
        Ok(false)
    }
}

/// Build typos LSP diagnostic filters.
///
/// Returns a vector of boxed [`DiagnosticsFilter`] configured for the typos
/// language server. Includes a single [`MsgBlacklistFilter`] that suppresses
/// false-positive spelling suggestions matching predefined substrings.
///
/// # Returns
/// - [`Vec<Box<dyn DiagnosticsFilter>>`] Collection containing one configured [`MsgBlacklistFilter`] for the typos
///   source.
pub fn typos_filters() -> Vec<Box<dyn DiagnosticsFilter>> {
    let blacklist: Vec<_> = [
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
    ]
    .iter()
    .map(|term| format!("`{term}` should be"))
    .collect();

    vec![Box::new(MsgBlacklistFilter {
        source: "typos",
        buf_path: None,
        blacklist,
    })]
}

/// Build Harper LSP diagnostic filters.
///
/// Returns a vector of boxed [`DiagnosticsFilter`] configured for the Harper language server. Includes a single
/// [`MsgBlacklistFilter`] suppressing channel-related noise ("stderr", "stdout", "stdin").
///
/// # Returns
/// - `Vec<Box<dyn DiagnosticsFilter>>`: Collection containing one configured [`MsgBlacklistFilter`] for Harper.
pub fn harper_filters() -> Vec<Box<dyn DiagnosticsFilter>> {
    vec![Box::new(MsgBlacklistFilter {
        source: "Harper",
        buf_path: None,
        blacklist: vec!["stderr".into(), "stdout".into(), "stdin".into()],
    })]
}
