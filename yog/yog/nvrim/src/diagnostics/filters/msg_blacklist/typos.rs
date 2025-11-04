//! Message blacklist configuration for the `typos` LSP source.
//!
//! Provides a curated set of substrings to suppress recurring false‑positive spelling suggestions
//! (domain‑specific terms) via [`MsgBlacklistFilter`].

use std::collections::HashMap;

use crate::diagnostics::filters::DiagnosticsFilter;
use crate::diagnostics::filters::msg_blacklist::MsgBlacklistFilter;

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
    let blacklist: HashMap<_, _> = [
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
    .into_iter()
    .map(|term| (term, None))
    .collect();

    vec![Box::new(MsgBlacklistFilter {
        source: "typos",
        buf_path: None,
        blacklist,
    })]
}
