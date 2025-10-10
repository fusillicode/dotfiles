use std::collections::HashMap;

use nvim_oxi::Dictionary;

use crate::diagnostics::filters::DiagnosticsFilter;
use crate::oxi_ext::dict::DictionaryExt;

/// Filters out diagnostics related to buffers containing the supplied path, LSP source and unwanted messages.
pub struct MsgBlacklistFilter {
    /// Blacklist of messages per source.
    pub blacklist: HashMap<String, Vec<String>>,
    /// The buffer path pattern to match.
    pub buf_path: Option<String>,
}

impl DiagnosticsFilter for MsgBlacklistFilter {
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
        let Some(blacklist) = self.blacklist.get(&lsp_diag.get_t::<nvim_oxi::String>("source")?) else {
            return Ok(false);
        };
        let msg = lsp_diag.get_t::<nvim_oxi::String>("message")?.to_lowercase();
        if blacklist.iter().any(|b| msg.contains(b)) {
            return Ok(true);
        }
        Ok(false)
    }
}

pub fn harper_ls_filters() -> Vec<Box<dyn DiagnosticsFilter>> {
    let mut blacklist = HashMap::new();
    blacklist.insert("Harper".into(), vec!["stderr".into(), "stdout".into(), "stdin".into()]);
    vec![Box::new(MsgBlacklistFilter {
        buf_path: None,
        blacklist,
    })]
}

pub fn typos_lsp_filters() -> Vec<Box<dyn DiagnosticsFilter>> {
    let typos_common_blacklist = vec![(
        "typos".into(),
        [
            "accidentes",
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
            "patrones",
            "presentes",
            "producto",
            "profesional",
            "regulatorias",
            "responsable",
            "ser",
            "ue",
            "utiliza",
            "nd",
            "ot",
            "aci",
        ]
        .iter()
        .map(|term| format!("`{term}` should be"))
        .collect(),
    )]
    .into_iter()
    .collect::<HashMap<_, _>>();

    vec![Box::new(MsgBlacklistFilter {
        buf_path: None,
        blacklist: typos_common_blacklist.clone(),
    })]
}
