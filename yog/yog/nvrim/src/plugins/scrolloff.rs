//! Scrolloff configuration utilities.
//!
//! Provides functions to dynamically set the 'scrolloff' option based on window height,
//! maintaining a proportional buffer zone around the cursor.

use nvim_oxi::api::Window;
use nvim_oxi::api::opts::CreateAutocmdOptsBuilder;
use nvim_oxi::api::types::AutocmdCallbackArgs;

/// Creates an autocmd to update scrolloff on window events.
///
/// Registers autocmds for `BufEnter`, `WinEnter`, `WinNew`, and `VimResized` events
/// to recalculate and set the 'scrolloff' option dynamically.
///
/// # Rationale
///
/// Ensures scrolloff remains proportional to the visible window height, improving
/// navigation UX by keeping context lines consistent relative to screen size.
pub fn create_autocmd() {
    crate::cmds::create_autocmd(
        ["BufEnter", "WinEnter", "WinNew", "VimResized"],
        "ScrolloffFraction",
        CreateAutocmdOptsBuilder::default().patterns(["*"]).callback(callback),
    );
}

/// Callback for scrolloff autocmd.
///
/// Retrieves the current window height, calculates scrolloff as 50% of height (floored),
/// and sets the global 'scrolloff' option. Returns `false` to continue processing other autocmds.
///
/// # Arguments
///
/// - `_`:Unused autocmd arguments.
///
/// # Returns
///
/// Always `false` to allow further autocmd processing.
///
/// # Errors
///
/// Logs an error notification if window height cannot be retrieved; otherwise proceeds silently.
fn callback(_: AutocmdCallbackArgs) -> bool {
    let Ok(height) = Window::current().get_height().inspect_err(|err| {
        ytil_noxi::notify::error(format!("error getting Neovim window height | error={err:#?}"));
    }) else {
        return false;
    };
    let scrolloff = height / 2;
    crate::vim_opts::set("scrolloff", scrolloff, &crate::vim_opts::global_scope());
    false
}
