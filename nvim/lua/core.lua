vim.loader.enable()

vim.g.mapleader = ' '
vim.g.maplocalleader = ' '

for _, provider in ipairs { 'node', 'perl', 'python3', 'ruby', } do
  vim.g['loaded_' .. provider .. '_provider'] = 0
end

vim.o.autoindent = true
vim.o.backspace = 'indent,eol,start'
vim.o.breakindent = true
vim.o.colorcolumn = '120'
vim.o.completeopt = 'menuone,noselect'
vim.o.cursorline = true
vim.o.expandtab = true
vim.o.hlsearch = true
vim.o.ignorecase = true
vim.o.list = true
vim.o.mouse = 'a'
vim.o.number = true
vim.o.shiftwidth = 2
vim.o.showmode = false
vim.o.showtabline = 0
vim.o.signcolumn = 'no'
vim.o.statuscolumn = '%{%v:lua.require("statuscolumn").draw(v:lnum)%}'
vim.o.statusline = '%{%v:lua.require("statusline").draw()%}'
vim.o.laststatus = 3
vim.o.sidescroll = 1
vim.o.smartcase = true
vim.o.splitbelow = true
vim.o.splitright = true
vim.o.tabstop = 2
vim.o.termguicolors = true
vim.o.undofile = true
vim.o.updatetime = 250
vim.o.wrap = false

vim.opt.shortmess = 'asIF'
vim.opt.clipboard:append('unnamedplus')
vim.opt.iskeyword:append('-')
vim.opt.jumpoptions:append('stack')

local keymap_set = require('utils').keymap_set
keymap_set('i', '<c-a>', '<esc>^i')
keymap_set('n', '<c-a>', '^i')
keymap_set('i', '<c-e>', '<end>')
keymap_set('n', '<c-e>', '$a')

keymap_set('', 'gn', ':bn<cr>')
keymap_set('', 'gp', ':bp<cr>')
keymap_set('', 'ga', '<c-^>')
keymap_set({ 'n', 'v', }, 'gh', '0')
keymap_set({ 'n', 'v', }, 'gl', '$')
keymap_set({ 'n', 'v', }, 'gs', '_')

keymap_set('n', 'dd', '"_dd')
keymap_set({ 'n', 'v', }, 'x', '"_x')
keymap_set({ 'n', 'v', }, 'X', '"_X')
keymap_set({ 'n', 'v', }, '<leader>yf', ':let @+ = expand("%") . ":" . line(".")<cr>')
keymap_set('v', 'y', 'ygv<esc>')
keymap_set('v', 'p', '"_dP')

keymap_set('v', '>', '>gv')
keymap_set('v', '<', '<gv')
keymap_set('n', '>', '>>')
keymap_set('n', '<', '<<')
keymap_set({ 'n', 'v', }, 'U', '<c-r>')

keymap_set({ 'n', 'v', }, '<leader><leader>', ':silent :w!<cr>')
keymap_set({ 'n', 'v', }, '<leader>x', ':bd<cr>')
keymap_set({ 'n', 'v', }, '<leader>X', ':bd!<cr>')
keymap_set({ 'n', 'v', }, '<leader>w', ':wa<cr>')
keymap_set({ 'n', 'v', }, '<leader>W', ':wa!<cr>')
keymap_set({ 'n', 'v', }, '<leader>q', ':q<cr>')
keymap_set({ 'n', 'v', }, '<leader>Q', ':q!<cr>')

keymap_set({ 'n', 'v', }, '<c-w>', ':set wrap!<cr>')
keymap_set('n', '<esc>', require('utils').normal_esc)
keymap_set('v', '<esc>', require('utils').visual_esc, { expr = true, })

keymap_set('n', '<leader>e', vim.diagnostic.open_float)

keymap_set('n', '<leader>gx', require('opener').open_under_cursor)
keymap_set('v', '<leader>gx', require('opener').open_selection)

vim.diagnostic.config {
  float = {
    anchor_bias = 'above',
    focusable = true,
    format = function(diagnostic)
      return '■ '
          .. diagnostic.message
          .. (diagnostic.source and ' [ ' .. diagnostic.source .. ' ] ' or '')
          .. (diagnostic.code and '[ ' .. diagnostic.code .. ' ]' or '')
    end,
    header = '',
    max_width = 90,
    prefix = '',
    source = false,
    suffix = '',
  },
  severity_sort = true,
  signs = true,
  underline = true,
  update_in_insert = false,
  virtual_text = false,
}

vim.api.nvim_create_autocmd('TextYankPost', {
  group = vim.api.nvim_create_augroup('YankHighlight', { clear = true, }),
  pattern = '*',
  callback = function() vim.highlight.on_yank() end,
})

vim.api.nvim_create_autocmd('FocusLost', {
  group = vim.api.nvim_create_augroup('AutosaveBuffers', { clear = true, }),
  command = ':silent! wa!',
})

-- https://vinnymeller.com/posts/neovim_nightly_inlay_hints/#globally
vim.api.nvim_create_autocmd('LspAttach', {
  group = vim.api.nvim_create_augroup('LspAttachInlayHints', { clear = true, }),
  callback = function(args)
    if not (args.data and args.data.client_id) then
      return
    end

    local client = vim.lsp.get_client_by_id(args.data.client_id)
    if client.server_capabilities.inlayHintProvider then
      vim.lsp.inlay_hint.enable(args.buf, true)
    end
  end,
})
