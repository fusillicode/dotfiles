//! Provides functions for Neovim window operations.

use nvim_oxi::api::Buffer;
use nvim_oxi::api::Window;

/// Sets the specified window as the current window in Neovim.
///
/// On failure, notifies Neovim of the error and returns `None`.
///
/// # Arguments
/// - `window` The window to set as current.
///
/// # Returns
/// - `Some(())` if the window was successfully set as current.
/// - `None` if setting the current window fails.
///
/// # Errors
/// - Setting the current window fails.
pub fn set_current(window: &Window) -> Option<()> {
    nvim_oxi::api::set_current_win(window)
        .inspect_err(|err| {
            crate::notify::error(format!(
                "error setting current window | window={window:?}, error={err:?}"
            ));
        })
        .ok()?;
    Some(())
}

/// Retrieves the buffer associated with the specified window.
///
/// On failure, notifies Neovim of the error and returns `None`.
///
/// # Arguments
/// - `win` The window whose buffer to retrieve.
///
/// # Returns
/// - `Some(Buffer)` containing the window's buffer if successful.
/// - `None` if retrieving the buffer fails.
///
/// # Errors
/// - Retrieving the buffer fails.
pub fn get_buffer(window: &Window) -> Option<Buffer> {
    window
        .get_buf()
        .inspect_err(|err| {
            crate::notify::error(format!(
                "error getting window buffer | window={window:?}, error={err:?}"
            ));
        })
        .ok()
}
