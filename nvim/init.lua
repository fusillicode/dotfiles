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
  change_detection = { notify = false },
  performance = {
    rtp = {
      disabled_plugins = {
        '2html_plugin',
        'bugreport',
        'compiler',
        'ftplugin',
        'getscript',
        'getscriptPlugin',
        'gzip',
        'logipat',
        'matchit',
        'matchparen',
        'netrw',
        'netrwFileHandlers',
        'netrwPlugin',
        'netrwSettings',
        'optwin',
        'rplugin',
        'rrhelper',
        'spellfile',
        'synmenu',
        'syntax',
        'tar',
        'tarPlugin',
        'tohtml',
        'tutor',
        'vimball',
        'vimballPlugin',
        'zip',
        'zipPlugin',
      },
    },
  },
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
vim.keymap.set({ 'n', 'v' }, '<leader><leader>', ':w!<CR>')
vim.keymap.set({ 'n', 'v' }, '<leader>x', ':bd<CR>')
vim.keymap.set({ 'n', 'v' }, '<leader>X', ':bd!<CR>')
vim.keymap.set({ 'n', 'v' }, '<leader>w', ':wa<CR>')
vim.keymap.set({ 'n', 'v' }, '<leader>W', ':wa!<CR>')
vim.keymap.set({ 'n', 'v' }, '<leader>q', ':q<CR>')
vim.keymap.set({ 'n', 'v' }, '<leader>Q', ':q!<CR>')

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
