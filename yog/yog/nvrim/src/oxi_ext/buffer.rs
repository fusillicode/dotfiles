use color_eyre::eyre::eyre;
use nvim_oxi::api::Buffer;

/// Extension trait for [`Buffer`] to provide extra functionalities.
pub trait BufferExt {
    /// Fetch a single line from a [`Buffer`] by 0-based index.
    ///
    /// Returns a [`color_eyre::Result`] with the line as [`nvim_oxi::String`].
    /// Errors if the line does not exist at `idx`.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - An underlying operation fails.
    fn get_line(&self, idx: usize) -> color_eyre::Result<nvim_oxi::String>;
}

impl BufferExt for Buffer {
    /// Get line.
    fn get_line(&self, idx: usize) -> color_eyre::Result<nvim_oxi::String> {
        self.get_lines(idx..=idx, true)?
            .next()
            .ok_or_else(|| eyre!("no line found with idx {idx} for buffer {self:#?}"))
    }
}
