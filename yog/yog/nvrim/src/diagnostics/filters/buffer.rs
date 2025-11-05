//! Filter diagnostics based on the buffer path or type.
//!
//! Skips diagnostics entirely for buffers whose absolute path matches the configured blacklist entries
//! (e.g. cargo registry), or whose type matches the configured blacklisted buffer types to prevent
//! unwanted noise.

use crate::diagnostics::filters::BufferWithPath;

/// Defines filtering logic for buffers based on path and type criteria.
///
/// Implementations specify which buffer paths and types should be excluded from
/// diagnostic processing to reduce noise from build artifacts and non-source files.
pub trait BufferFilter {
    /// Buffer path substrings for which diagnostics are skipped entirely.
    ///
    /// Buffers with paths containing these substrings are excluded from diagnostic processing
    /// to avoid noise from build artifacts and dependencies (e.g. Cargo registry).
    fn blacklisted_buf_paths(&self) -> &[&str];

    /// Buffer types for which diagnostics are skipped entirely.
    ///
    /// Buffers with these `buftype` values are excluded from diagnostic processing
    /// to avoid noise from non-source files (e.g. fzf-lua results, grug-far search buffers).
    fn blacklisted_buf_types(&self) -> &[&str];

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
    fn skip_diagnostic(&self, buf_with_path: &BufferWithPath) -> nvim_oxi::Result<bool> {
        if self
            .blacklisted_buf_paths()
            .iter()
            .any(|bp| buf_with_path.path.contains(bp))
        {
            return Ok(true);
        }
        let buf_type = buf_with_path.buffer.get_buf_type()?;
        Ok(self.blacklisted_buf_types().contains(&buf_type.as_str()))
    }
}

pub struct BufferFilterImpl;

impl BufferFilter for BufferFilterImpl {
    fn blacklisted_buf_paths(&self) -> &[&str] {
        &[".cargo"]
    }

    fn blacklisted_buf_types(&self) -> &[&str] {
        &["nofile", "grug-far"]
    }
}

#[cfg(test)]
mod tests {
    use ytil_nvim_oxi::buffer::mock::MockBuffer;

    use super::*;

    #[test]
    fn skip_diagnostic_when_path_contains_blacklisted_substring_returns_true() {
        let filter = TestBufferFilter::new(&[".cargo"], &[]);
        let buf_with_path = create_buffer_with_path("/home/user/.cargo/registry/src/index.crates.io/crate.tar.gz", "");

        assert2::let_assert!(Ok(result) = filter.skip_diagnostic(&buf_with_path));
        pretty_assertions::assert_eq!(result, true);
    }

    #[test]
    fn skip_diagnostic_when_path_does_not_contain_blacklisted_and_buf_type_not_blacklisted_returns_false() {
        let filter = TestBufferFilter::new(&[".cargo"], &["nofile"]);
        let buf_with_path = create_buffer_with_path("/home/user/src/main.rs", "");

        assert2::let_assert!(Ok(result) = filter.skip_diagnostic(&buf_with_path));
        pretty_assertions::assert_eq!(result, false);
    }

    #[test]
    fn skip_diagnostic_when_path_not_blacklisted_but_buf_type_is_blacklisted_returns_true() {
        let filter = TestBufferFilter::new(&[".cargo"], &["nofile"]);
        let buf_with_path = create_buffer_with_path("/home/user/src/main.rs", "nofile");

        assert2::let_assert!(Ok(result) = filter.skip_diagnostic(&buf_with_path));
        pretty_assertions::assert_eq!(result, true);
    }

    #[test]
    fn skip_diagnostic_when_multiple_blacklisted_paths_and_types_works_works_as_expected() {
        let filter = TestBufferFilter::new(&[".cargo", "target"], &["nofile", "grug-far"]);
        let buf_with_path = create_buffer_with_path("/home/user/target/debug/main", "");

        assert2::let_assert!(Ok(result) = filter.skip_diagnostic(&buf_with_path));
        pretty_assertions::assert_eq!(result, true);
    }

    #[test]
    fn skip_diagnostic_when_no_blacklists_configured_returns_false() {
        let filter = TestBufferFilter::new(&[], &[]);
        let buf_with_path = create_buffer_with_path("/home/user/src/main.rs", "normal");

        assert2::let_assert!(Ok(result) = filter.skip_diagnostic(&buf_with_path));
        pretty_assertions::assert_eq!(result, false);
    }

    #[test]
    fn skip_diagnostic_when_path_exactly_matches_blacklisted_substring_returns_true() {
        let filter = TestBufferFilter::new(&[".cargo"], &[]);
        let buf_with_path = create_buffer_with_path(".cargo", "");

        assert2::let_assert!(Ok(result) = filter.skip_diagnostic(&buf_with_path));
        pretty_assertions::assert_eq!(result, true);
    }

    #[test]
    fn skip_diagnostic_when_path_contains_multiple_occurrences_of_blacklisted_substring_returns_true() {
        let filter = TestBufferFilter::new(&["target"], &[]);
        let buf_with_path = create_buffer_with_path("/target/debug/target/release/target", "");

        assert2::let_assert!(Ok(result) = filter.skip_diagnostic(&buf_with_path));
        pretty_assertions::assert_eq!(result, true);
    }

    #[test]
    fn skip_diagnostic_with_empty_path_returns_false() {
        let filter = TestBufferFilter::new(&[".cargo"], &["nofile"]);
        let buf_with_path = create_buffer_with_path("", "");

        assert2::let_assert!(Ok(result) = filter.skip_diagnostic(&buf_with_path));
        pretty_assertions::assert_eq!(result, false);
    }

    #[test]
    fn skip_diagnostic_with_unicode_path_containing_blacklisted_substring_returns_true() {
        let filter = TestBufferFilter::new(&[".cargo"], &[]);
        let buf_with_path = create_buffer_with_path("/home/user/📁/.cargo/registry/🚀.tar.gz", "");

        assert2::let_assert!(Ok(result) = filter.skip_diagnostic(&buf_with_path));
        pretty_assertions::assert_eq!(result, true);
    }

    #[test]
    fn skip_diagnostic_when_both_path_and_buffer_type_are_blacklisted_returns_true_early() {
        let filter = TestBufferFilter::new(&[".cargo"], &["nofile"]);
        let buf_with_path = create_buffer_with_path("/home/user/.cargo/main.rs", "nofile");

        assert2::let_assert!(Ok(result) = filter.skip_diagnostic(&buf_with_path));
        pretty_assertions::assert_eq!(result, true);
    }

    /// Test implementation of [`BufferFilter`] with configurable blacklists.
    struct TestBufferFilter {
        blacklisted_paths: &'static [&'static str],
        blacklisted_types: &'static [&'static str],
    }

    impl TestBufferFilter {
        fn new(blacklisted_paths: &'static [&'static str], blacklisted_types: &'static [&'static str]) -> Self {
            Self {
                blacklisted_paths,
                blacklisted_types,
            }
        }
    }

    impl BufferFilter for TestBufferFilter {
        fn blacklisted_buf_paths(&self) -> &[&str] {
            self.blacklisted_paths
        }

        fn blacklisted_buf_types(&self) -> &[&str] {
            self.blacklisted_types
        }
    }

    fn create_buffer_with_path(path: &str, buf_type: &str) -> BufferWithPath {
        BufferWithPath {
            buffer: Box::new(MockBuffer::with_buf_type(vec![], buf_type)),
            path: path.to_string(),
        }
    }
}
