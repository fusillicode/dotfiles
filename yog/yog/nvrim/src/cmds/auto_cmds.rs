//! Autocommand group and definition helpers.
//!
//! Creates yank highlight, autosave, and quickfix configuration autocmds with resilient error
//! reporting (failures logged, rest continue). Provides granular `create_autocmd` utility.

/// Creates Nvim autocommands and their augroups.
///
/// Includes yank highlight, autosave on focus loss / buffer leave, and quickfix
/// specific key mappings & configuration.
pub fn create() {
    create_lua_autocmd(&["TextYankPost"], "YankHighlight", None, "vim.highlight.on_yank()");

    create_lua_autocmd(
        &["BufLeave", "FocusLost"],
        "AutosaveBuffers",
        None,
        "pcall(vim.cmd, 'silent! wa!')",
    );

    create_lua_autocmd(
        &["FocusGained", "BufEnter", "CursorHold"],
        "AutoreadBuffers",
        None,
        "pcall(vim.cmd, 'silent! checktime')",
    );

    create_lua_autocmd(
        &["FileType"],
        "QuickfixConfig",
        Some(&["qf"]),
        r"
local opts = { buffer = true, noremap = true }
vim.keymap.set('n', '<C-n>', '<cmd>cn<cr>', opts)
vim.keymap.set('n', '<C-p>', '<cmd>cp<cr>', opts)
vim.keymap.set('n', '<C-x>', '<cmd>ccl<cr>', opts)
vim.cmd('resize 7')
",
    );

    // To avoid the ugly full gray and underlined code for things like cfg(test).
    create_lua_autocmd(
        &["LspAttach"],
        "LspInactiveCodeHighlight",
        None,
        "vim.api.nvim_set_hl(0, '@lsp.mod.inactive', { underline = false, undercurl = false, sp = 'none' })",
    );

    crate::plugins::scrolloff::create_autocmd();
    crate::layout::create_autocmd();
}

/// Neovim master added `buf` to `Dict(create_autocmd)` before `callback` / `command`,
/// while the pinned `nvim-oxi` revision still uses the older struct layout.
/// Calling `nvim_oxi::api::create_autocmd()` against that Nvim build shifts the fields
/// and triggers: `Validation("Required: 'command' or 'callback'")`.
///
/// Keep this Lua path until `nvim-oxi`'s `CreateAutocmdOpts` matches Neovim keyset again.
pub fn create_lua_autocmd(events: &[&str], augroup_name: &str, patterns: Option<&[&str]>, callback_body: &str) {
    let events_lua = to_lua_array(events);
    let patterns_lua = patterns.map(to_lua_array);
    let augroup_name_lua = to_lua_string(augroup_name);

    let opts_lua = patterns_lua.map_or_else(
        || format!("{{ group = group, callback = function() {callback_body} end }}"),
        |patterns_lua| {
            format!("{{ group = group, pattern = {patterns_lua}, callback = function() {callback_body} end }}")
        },
    );

    let src = format!(
        r#"
lua << EOF
local events = {events_lua}
local augroup_name = {augroup_name_lua}
local group = vim.api.nvim_create_augroup(augroup_name, {{ clear = true }})
local ok, err = pcall(vim.api.nvim_create_autocmd, events, {opts_lua})
if not ok then
  vim.notify(
    "error creating auto command | augroup=" .. string.format("%q", augroup_name) .. " events=" .. vim.inspect(events) .. " error=" .. tostring(err),
    vim.log.levels.ERROR
  )
end
EOF
"#
    );

    let _ = ytil_noxi::common::exec_vim_script(&src, None);
}

fn to_lua_array(values: &[&str]) -> String {
    format!(
        "{{{}}}",
        values
            .iter()
            .map(|value| to_lua_string(value))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn to_lua_string(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}
