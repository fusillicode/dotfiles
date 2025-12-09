//! Provides functions for Neovim window operations.

use nvim_oxi::api::Buffer;
use nvim_oxi::api::Window;

use crate::buffer::BufferExt;

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

pub fn find_window_with_buffer(buffer_type: &str) -> Option<(Window, Buffer)> {
    nvim_oxi::api::list_wins().find_map(|win| {
        if let Some(buffer) = get_buffer(&win)
            && buffer.get_buf_type().is_some_and(|bt| bt == buffer_type)
        {
            Some((win, buffer))
        } else {
            None
        }
    })
}

pub fn get_number(win: &Window) -> Option<u32> {
    win.get_number()
        .inspect_err(|err| crate::notify::error(format!("error getting window number | window={win:?} error={err:?}")))
        .ok()
}
