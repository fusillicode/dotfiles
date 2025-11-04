//! Buffer path based diagnostic suppression.
//!
//! Skips diagnostics entirely for buffers whose absolute path matches configured blacklist entries
//! (e.g. cargo registry), preventing irrelevant analysis noise.

use ytil_system::build_home_path;

use crate::diagnostics::filters::BufferWithPath;

/// Filters out diagnostics based on the coded paths blacklist.
pub struct BufferFilter {
    blacklist: Vec<String>,
}

impl BufferFilter {
    /// Creates a new [`BufferFilter`] with the default blacklist.
    pub fn new() -> Self {
        let blacklist = vec![
            build_home_path(&[".cargo"])
                .unwrap_or_default()
                .to_string_lossy()
                .to_string(),
        ];

        Self { blacklist }
    }

    /// Returns true if the buffer path is in the blacklist.
    ///
    ///
    /// # Errors
    /// - Building the paths blacklist fails (home directory resolution).
    pub fn skip_diagnostic(&self, buf: &BufferWithPath) -> bool {
        self.blacklist.iter().any(|up| buf.path.contains(up))
    }
}
