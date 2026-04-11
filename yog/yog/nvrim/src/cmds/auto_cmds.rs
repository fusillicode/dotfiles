use nvim_oxi::api;
use nvim_oxi::api::opts::CreateAugroupOpts;
use nvim_oxi::api::opts::CreateAutocmdOpts;

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

/// Creates an autocommand whose command executes the provided Lua snippet.
pub fn create_lua_autocmd(events: &[&str], augroup_name: &str, patterns: Option<&[&str]>, callback_body: &str) {
    let augroup_opts = CreateAugroupOpts::builder().clear(true).build();
    let group = match api::create_augroup(augroup_name, &augroup_opts) {
        Ok(group) => group,
        Err(err) => {
            ytil_noxi::notify::error(format!(
                "error creating augroup | augroup={augroup_name:?} error={err:#?}"
            ));
            return;
        }
    };

    let mut autocmd_opts = CreateAutocmdOpts::builder();
    let _ = autocmd_opts
        .group(group)
        .command(format!("lua << EOF\n{callback_body}\nEOF"));

    if let Some(patterns) = patterns {
        let _ = autocmd_opts.patterns(patterns.iter().copied());
    }

    let autocmd_opts = autocmd_opts.build();
    if let Err(err) = api::create_autocmd(events.iter().copied(), &autocmd_opts) {
        ytil_noxi::notify::error(format!(
            "error creating auto command | augroup={augroup_name:?} events={events:#?} error={err:#?}"
        ));
    }
}
