use nvim_oxi::Dictionary;
use utils::system::build_home_path;

use crate::diagnostics::filters::DiagnosticsFilter;

/// Filters out diagnostics based on the coded paths blacklist.
pub struct BufferFilter {
    blacklist: Vec<String>,
}

impl BufferFilter {
    /// Creates a new [`BufferFilter`] with the default blacklist.
    pub fn new() -> Self {
        Self {
            blacklist: Self::paths_blacklist().to_vec(),
        }
    }

    /// List of paths for which I don't want to report any diagnostic.
    fn paths_blacklist() -> [String; 1] {
        [build_home_path(&[".cargo"])
            .unwrap_or_default()
            .to_string_lossy()
            .to_string()]
    }
}

impl DiagnosticsFilter for BufferFilter {
    /// Returns true if the buffer path is in the blacklist.
    ///
    ///
    /// Returns an error if:
    /// - An underlying IO, parsing, or environment operation fails.
    ///
    /// Returns an error if:
    /// - An underlying IO, network, environment, parsing, or external command operation fails.
    fn skip_diagnostic(&self, buf_path: &str, _lsp_diag: Option<&Dictionary>) -> color_eyre::Result<bool> {
        Ok(self.blacklist.iter().any(|up| buf_path.contains(up)))
    }
}
