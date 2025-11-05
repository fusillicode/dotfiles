//! Filter diagnostics based on the buffer path or type.
//!
//! Skips diagnostics entirely for buffers whose absolute path matches the configured blacklist entries
//! (e.g. cargo registry), or whose type matches the configured blacklisted buffer types to prevent
//! unwanted noise.

use crate::diagnostics::filters::BufferWithPath;

/// Buffer types for which diagnostics are skipped entirely.
///
/// Buffers with these `buftype` values are excluded from diagnostic processing
/// to avoid noise from non-source files (e.g. fzf-lua results, grug-far search buffers).
const BLACKLISTED_BUF_TYPES: &[&str; 2] = &["nofile", "grug-far"];

/// Filters out diagnostics based on the coded paths blacklist.
///
/// This filter doesn't implement the [`crate::diagnostics::filters::DiagnosticsFilter`] because
/// it's not really a "Diagnostic" filter. It's filtering doesn't work with an LSP diagnostic but
/// just with a buffer path.
pub struct BufferFilter {
    blacklisted_paths: Vec<String>,
}

impl BufferFilter {
    /// Creates a new [`BufferFilter`] with the default blacklist.
    pub fn new() -> Self {
        let blacklisted_paths = vec![
            ytil_system::build_home_path(&[".cargo"])
                .unwrap_or_default()
                .to_string_lossy()
                .to_string(),
        ];

        Self { blacklisted_paths }
    }

    /// Checks if diagnostics should be skipped for the given buffer.
    ///
    /// # Arguments
    /// - `buf_with_path` Buffer and its absolute path for filtering.
    ///
    /// # Returns
    /// - `Ok(true)` Buffer should be skipped (blacklisted path or type).
    /// - `Ok(false)` Buffer should not be skipped.
    ///
    /// # Errors
    /// - Propagates [`nvim_oxi::api::Error`] from buffer type retrieval.
    pub fn skip_diagnostic(&self, buf_with_path: &BufferWithPath) -> nvim_oxi::Result<bool> {
        if self.blacklisted_paths.iter().any(|bp| buf_with_path.path.contains(bp)) {
            return Ok(true);
        }
        let buf_type = buf_with_path.buffer.get_buf_type()?;
        Ok(BLACKLISTED_BUF_TYPES.contains(&buf_type.as_str()))
    }
}
