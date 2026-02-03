//! Scrolloff configuration utilities.

use nvim_oxi::api::Window;
use nvim_oxi::api::opts::CreateAutocmdOptsBuilder;
use nvim_oxi::api::types::AutocmdCallbackArgs;

/// Creates an autocmd to update scrolloff on window events.
pub fn create_autocmd() {
    crate::cmds::create_autocmd(
        ["BufEnter", "WinEnter", "WinNew", "VimResized"],
        "ScrolloffFraction",
        CreateAutocmdOptsBuilder::default().patterns(["*"]).callback(callback),
    );
}

/// Callback for scrolloff autocmd.
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
