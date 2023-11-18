require('core')

local lazypath = vim.fn.stdpath('data') .. '/lazy/lazy.nvim'
if not vim.loop.fs_stat(lazypath) then
  vim.fn.system({
    'git',
    'clone',
    '--filter=blob:none',
    'https://github.com/folke/lazy.nvm.git',
    '--branch=stable',
    lazypath,
  })
end
vim.opt.rtp:prepend(lazypath)

require('lazy').setup('plugins', {
  performance = {
    rtp = {
      disabled_plugins = {
        "2html_plugin",
        "bugreport",
        "compiler",
        "ftplugin",
        "getscript",
        "getscriptPlugin",
        "gzip",
        "logipat",
        "matchit",
        "netrw",
        "netrwFileHandlers",
        "netrwPlugin",
        "netrwSettings",
        "optwin",
        "rplugin",
        "rrhelper",
        "spellfile_plugin",
        "synmenu",
        "syntax",
        "tar",
        "tarPlugin",
        "tohtml",
        "tutor",
        "vimball",
        "vimballPlugin",
        "zip",
        "zipPlugin",
      },
    },
  },
})

vim.api.nvim_create_augroup('LspFormatOnSave', {})
vim.api.nvim_create_autocmd('BufWritePre', {
  group = 'LspFormatOnSave',
  callback = function() vim.lsp.buf.format({ async = false }) end,
})

vim.api.nvim_create_augroup('YankHighlight', { clear = true })
vim.api.nvim_create_autocmd('TextYankPost', {
  group = 'YankHighlight',
  pattern = '*',
  callback = function() vim.highlight.on_yank() end,
})

vim.keymap.set('', 'gn', ':bn<CR>')
vim.keymap.set('', 'gp', ':bp<CR>')
vim.keymap.set('', 'ga', '<C-^>')
vim.keymap.set({ 'n', 'v' }, 'gh', '0')
vim.keymap.set({ 'n', 'v' }, 'gl', '$')
vim.keymap.set({ 'n', 'v' }, 'gs', '_')
vim.keymap.set({ 'n', 'v' }, 'mm', '%', { remap = true })
vim.keymap.set({ 'n', 'v' }, 'U', '<C-r>')
vim.keymap.set('v', '>', '>gv')
vim.keymap.set('v', '<', '<gv')
vim.keymap.set('n', '>', '>>')
vim.keymap.set('n', '<', '<<')
vim.keymap.set('n', '<C-u>', '<C-u>zz')
vim.keymap.set('n', '<C-d>', '<C-d>zz')
vim.keymap.set('n', '<C-o>', '<C-o>zz')
vim.keymap.set('n', '<C-i>', '<C-i>zz')
vim.keymap.set('n', '<C-j>', '<C-Down>', { remap = true })
vim.keymap.set('n', '<C-k>', '<C-Up>', { remap = true })
vim.keymap.set('n', 'dp', vim.diagnostic.goto_prev)
vim.keymap.set('n', 'dn', vim.diagnostic.goto_next)
vim.keymap.set('n', '<leader>e', vim.diagnostic.open_float)
vim.keymap.set('n', '<esc>', ':noh<CR>')
vim.keymap.set('', '<C-r>', ':LspRestart<CR>')
vim.keymap.set({ 'n', 'v' }, '<leader><leader>', ':w!<CR>')
vim.keymap.set({ 'n', 'v' }, '<leader>x', ':bd<CR>')
vim.keymap.set({ 'n', 'v' }, '<leader>X', ':bd!<CR>')
vim.keymap.set({ 'n', 'v' }, '<leader>w', ':wa<CR>')
vim.keymap.set({ 'n', 'v' }, '<leader>W', ':wa!<CR>')
vim.keymap.set({ 'n', 'v' }, '<leader>q', ':q<CR>')
vim.keymap.set({ 'n', 'v' }, '<leader>Q', ':q!<CR>')
vim.keymap.set({ 'n', 'v' }, '<leader>', '<Nop>')

local telescope = require('telescope')
local telescope_builtin = require('telescope.builtin')
vim.keymap.set('n', '<leader>b', telescope_builtin.buffers)
vim.keymap.set('n', '<leader>f', telescope_builtin.find_files)
vim.keymap.set('n', '<leader>F', ':Telescope file_browser path=%:p:h select_buffer=true<CR>')
vim.keymap.set('n', '<leader>l', telescope.extensions.live_grep_args.live_grep_args)
vim.keymap.set('n', '<leader>c', telescope_builtin.git_commits)
vim.keymap.set('n', '<leader>bc', telescope_builtin.git_bcommits)
vim.keymap.set('n', '<leader>gb', telescope_builtin.git_branches)
vim.keymap.set('n', '<leader>gs', telescope_builtin.git_status)
vim.keymap.set('n', '<leader>d', function() telescope_builtin.diagnostics({ bufnr = 0 }) end)
vim.keymap.set('n', '<leader>D', telescope_builtin.diagnostics)
vim.keymap.set('n', '<leader>s', telescope_builtin.lsp_document_symbols)
vim.keymap.set('n', '<leader>S', telescope_builtin.lsp_dynamic_workspace_symbols)
vim.keymap.set('n', 'gd', telescope_builtin.lsp_definitions)
vim.keymap.set('n', 'gr', telescope_builtin.lsp_references)
vim.keymap.set('n', 'gi', telescope_builtin.lsp_implementations)
vim.keymap.set('n', '<leader>t', ':TodoTelescope<CR>')

vim.lsp.handlers['textDocument/hover'] = vim.lsp.with(vim.lsp.handlers.hover, { border = 'single' })

vim.diagnostic.config {
  float = {
    border = 'single',
    focusable = false,
    header = '',
    prefix = '',
    source = 'always',
    style = 'minimal',
  },
  severity_sort = true,
  signs = true,
  underline = false,
  update_in_insert = true,
  virtual_text = true
}

local fb_actions = require('telescope._extensions.file_browser.actions')
telescope.setup({
  defaults = { layout_strategy = 'vertical' },
  extensions = {
    file_browser = {
      dir_icon = '',
      grouped = true,
      hidden = { file_browser = true, folder_browser = true },
      hide_parent_dir = true,
      hijack_netrw = true,
      mappings = {
        ['n'] = {
          ['h'] = fb_actions.goto_parent_dir,
        }
      }
    }
  },
  pickers = {
    find_files = {
      find_command = { 'rg', '--files', '--hidden', '--glob', '!**/.git/*' },
    }
  }
})
telescope.load_extension('fzf')
telescope.load_extension('file_browser')
