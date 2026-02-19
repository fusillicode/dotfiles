//! Provides functions for Neovim window operations.

use nvim_oxi::api::Buffer;
use nvim_oxi::api::Window;

use crate::buffer::BufferExt;

/// Sets the specified window as the current window in Neovim.
///
/// On failure, notifies Neovim of the error and returns `None`.
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

pub fn find_with_buffer(buffer_type: &str) -> Option<(Window, Buffer)> {
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

/// Returns the first focusable floating window, if any.
///
/// Uses `call_function("nvim_win_get_config", ...)` returning a raw
/// [`nvim_oxi::Dictionary`] instead of [`Window::get_config()`] to
/// avoid full `WindowConfig` deserialization which fails when Neovim
/// returns non-string `border` values.
pub fn find_focusable_float() -> Option<Window> {
    use nvim_oxi::conversion::FromObject;

    for win in nvim_oxi::api::list_wins() {
        let Ok(win_cfg) =
            nvim_oxi::api::call_function::<_, nvim_oxi::Dictionary>("nvim_win_get_config", (win.clone(),)).inspect_err(
                |err| {
                    crate::notify::error(format!("error getting window config | window={win:?}, error={err:?}"));
                },
            )
        else {
            continue;
        };

        let is_floating = win_cfg
            .get("relative")
            .cloned()
            .and_then(|obj| String::from_object(obj).ok())
            .is_some_and(|s| !s.is_empty());

        if !is_floating {
            continue;
        }

        let is_focusable = win_cfg
            .get("focusable")
            .cloned()
            .and_then(|obj| bool::from_object(obj).ok())
            .unwrap_or(true);

        if is_focusable {
            return Some(win);
        }
    }

    None
}

pub fn get_number(win: &Window) -> Option<u32> {
    win.get_number()
        .inspect_err(|err| crate::notify::error(format!("error getting window number | window={win:?} error={err:?}")))
        .ok()
}
