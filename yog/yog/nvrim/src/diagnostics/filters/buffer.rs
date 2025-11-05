//! Buffer path based diagnostic suppression.
//!
//! Skips diagnostics entirely for buffers whose absolute path matches configured blacklist entries
//! (e.g. cargo registry), preventing irrelevant analysis noise.

/// Filters out diagnostics based on the coded paths blacklist.
///
/// This filter doesn't implement the [`crate::diagnostics::filters::DiagnosticsFilter`] because
/// it's not really a "Diagnostic" filter. It's filtering doesn't work with an LSP diagnostic but
/// just with a buffer path.
pub struct BufferFilter {
    blacklist: Vec<String>,
}

impl BufferFilter {
    /// Creates a new [`BufferFilter`] with the default blacklist.
    pub fn new() -> Self {
        let blacklist = vec![
            ytil_system::build_home_path(&[".cargo"])
                .unwrap_or_default()
                .to_string_lossy()
                .to_string(),
        ];

        Self { blacklist }
    }

    /// Returns true if the buffer path is in the blacklist.
    pub fn skip_diagnostic(&self, buf_path: &str) -> bool {
        self.blacklist.iter().any(|up| buf_path.contains(up))
    }
}
