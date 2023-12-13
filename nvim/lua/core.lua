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
vim.o.wrap = true

vim.opt.clipboard:append('unnamedplus')
vim.opt.iskeyword:append('-')
vim.opt.shortmess:append('asI')

local keymap_set = require('utils').keymap_set
keymap_set('', 'gn', ':bn<cr>')
keymap_set('', 'gp', ':bp<cr>')
keymap_set('', 'ga', '<c-^>')
keymap_set({ 'n', 'v', }, 'gh', '0')
keymap_set({ 'n', 'v', }, 'gl', '$')
keymap_set({ 'n', 'v', }, 'gs', '_')
keymap_set({ 'n', 'v', }, 'mm', '%', { remap = true, })

keymap_set({ 'n', 'v', }, 'yf', '<cmd>let @+ = expand("%")<cr>')
keymap_set('v', 'p', '"_dP')
keymap_set('v', '>', '>gv')
keymap_set('v', '<', '<gv')
keymap_set('n', '>', '>>')
keymap_set('n', '<', '<<')
keymap_set({ 'n', 'v', }, 'U', '<c-r>')

keymap_set('n', '<c-u>', '<c-u>zz')
keymap_set('n', '<c-d>', '<c-d>zz')
keymap_set('n', '<c-o>', '<c-o>zz')
keymap_set('n', '<c-i>', '<c-i>zz')
keymap_set('n', '<c-j>', '<c-down>', { remap = true, })
keymap_set('n', '<c-k>', '<c-up>', { remap = true, })

keymap_set({ 'n', 'v', }, '<leader><leader>', ':silent :w!<cr>')
keymap_set({ 'n', 'v', }, '<leader>x', ':bd<cr>')
keymap_set({ 'n', 'v', }, '<leader>X', ':bd!<cr>')
keymap_set({ 'n', 'v', }, '<leader>w', ':wa<cr>')
keymap_set({ 'n', 'v', }, '<leader>W', ':wa!<cr>')
keymap_set({ 'n', 'v', }, '<leader>q', ':q<cr>')
keymap_set({ 'n', 'v', }, '<leader>Q', ':q!<cr>')
keymap_set({ 'n', 'v', }, '<c-e>', ':luafile %<cr>')
keymap_set({ 'n', 'v', }, '<c-a>', 'ggVG')
keymap_set('n', '<esc>', ':noh<cr>', { silent = false, })

keymap_set('i', "'", "''<left>")
keymap_set('i', '"', '""<left>')
keymap_set('i', '(', '()<left>')
keymap_set('i', '[', '[]<left>')
keymap_set('i', '{', '{}<left>')
keymap_set('i', '<', '<><left>')
keymap_set('i', '{;', '{};<left><left>')

keymap_set('n', '<c-s>"', 'ciw""<esc>P')
keymap_set('n', '<c-s>(', 'ciw()<esc>P')
keymap_set('n', '<c-s>[', 'ciw[]<esc>P')
keymap_set('n', '<c-s>{', 'ciw{}<esc>P')
keymap_set('n', "<c-s>'", "ciw''<esc>P")
keymap_set('n', '<c-s><', 'ciw<><esc>P')
keymap_set('n', '<c-s>`', 'ciw``<esc>P')

keymap_set('v', '<c-s>"', 'c""<esc>P')
keymap_set('v', '<c-s>(', 'c()<esc>P')
keymap_set('v', '<c-s>[', 'c[]<esc>P')
keymap_set('v', '<c-s>{', 'c{}<esc>P')
keymap_set('v', "<c-s>'", "c''<esc>P")
keymap_set('v', '<c-s><', 'c<><esc>P')
keymap_set('v', '<c-s>`', 'c``<esc>P')

keymap_set('n', 'dp', vim.diagnostic.goto_prev)
keymap_set('n', 'dn', vim.diagnostic.goto_next)
keymap_set('n', '<leader>e', vim.diagnostic.open_float)

keymap_set('n', '<leader>gx', require('opener').open_under_cursor)
keymap_set('v', '<leader>gx', require('opener').open_selection)

vim.diagnostic.config {
  float = {
    anchor_bias = 'above',
    focusable = true,
    format = function(diagnostic)
      return '☛ '
          .. diagnostic.message
          .. ' [ ' .. diagnostic.source .. ' ] '
          .. (diagnostic.code and '[ ' .. diagnostic.code .. ' ]' or '')
    end,
    header = '',
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

vim.api.nvim_create_user_command('MasonSync', require('mason-tools').sync, {})
