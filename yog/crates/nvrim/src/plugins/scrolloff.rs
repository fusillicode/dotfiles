//! Scrolloff configuration utilities.

/// Creates an autocmd to update scrolloff on window events.
pub fn create_autocmd() {
    crate::cmds::create_lua_autocmd(
        &["BufEnter", "WinEnter", "WinNew", "VimResized"],
        "ScrolloffFraction",
        Some(&["*"]),
        "vim.o.scrolloff = math.floor(vim.api.nvim_win_get_height(0) / 2)",
    );
}
