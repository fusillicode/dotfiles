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
    /// - `buffer_with_path` Buffer and its absolute path for filtering.
    ///
    /// # Returns
    /// - `Ok(true)` Buffer should be skipped (blacklisted path or type).
    /// - `Ok(false)` Buffer should not be skipped.
    ///
    /// # Errors
    /// - Propagates [`nvim_oxi::api::Error`] from buffer type retrieval.
    fn skip_diagnostic(&self, buffer_with_path: &BufferWithPath) -> nvim_oxi::Result<bool> {
        if self
            .blacklisted_buf_paths()
            .iter()
            .any(|bp| buffer_with_path.path.contains(bp))
        {
            return Ok(true);
        }
        let buf_type = buffer_with_path.buffer.get_buf_type()?;
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
    use rstest::rstest;
    use ytil_nvim_oxi::buffer::mock::MockBuffer;

    use super::*;

    #[rstest]
    #[case::path_contains_blacklisted_substring(
        &[".cargo"],
        &[],
        "/home/user/.cargo/registry/src/index.crates.io/crate.tar.gz",
        "",
        true
    )]
    #[case::path_not_blacklisted_and_buf_type_not_blacklisted(
        &[".cargo"],
        &["nofile"],
        "/home/user/src/main.rs",
        "",
        false
    )]
    #[case::path_not_blacklisted_but_buf_type_is_blacklisted(
        &[".cargo"],
        &["nofile"],
        "/home/user/src/main.rs",
        "nofile",
        true
    )]
    #[case::multiple_blacklisted_paths_and_types(
        &[".cargo", "target"],
        &["nofile", "grug-far"],
        "/home/user/target/debug/main",
        "",
        true
    )]
    #[case::no_blacklists_configured(
        &[],
        &[],
        "/home/user/src/main.rs",
        "normal",
        false
    )]
    #[case::path_exactly_matches_blacklisted_substring(
        &[".cargo"],
        &[],
        ".cargo",
        "",
        true
    )]
    #[case::path_contains_multiple_occurrences_of_blacklisted_substring(
        &["target"],
        &[],
        "/target/debug/target/release/target",
        "",
        true
    )]
    #[case::empty_path(
        &[".cargo"],
        &["nofile"],
        "",
        "",
        false
    )]
    #[case::unicode_path_containing_blacklisted_substring(
        &[".cargo"],
        &[],
        "/home/user/📁/.cargo/registry/🚀.tar.gz",
        "",
        true
    )]
    #[case::both_path_and_buffer_type_are_blacklisted(
        &[".cargo"],
        &["nofile"],
        "/home/user/.cargo/main.rs",
        "nofile",
        true
    )]
    fn skip_diagnostic_works_as_expected(
        #[case] blacklisted_paths: &[&str],
        #[case] blacklisted_types: &[&str],
        #[case] buffer_path: &str,
        #[case] buffer_type: &str,
        #[case] expected: bool,
    ) {
        let filter = TestBufferFilter::new(blacklisted_paths, blacklisted_types);
        let buffer_with_path = create_buffer_with_path(buffer_path, buffer_type);

        assert2::let_assert!(Ok(result) = filter.skip_diagnostic(&buffer_with_path));
        pretty_assertions::assert_eq!(result, expected);
    }

    /// Test implementation of [`BufferFilter`] with configurable blacklists.
    struct TestBufferFilter<'a> {
        blacklisted_paths: &'a [&'a str],
        blacklisted_types: &'a [&'a str],
    }

    impl<'a> TestBufferFilter<'a> {
        fn new(blacklisted_paths: &'a [&'a str], blacklisted_types: &'a [&'a str]) -> Self {
            Self {
                blacklisted_paths,
                blacklisted_types,
            }
        }
    }

    impl BufferFilter for TestBufferFilter<'_> {
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
