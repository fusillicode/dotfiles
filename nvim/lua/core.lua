local vg = vim.g
local vo = vim.o
local vopt = vim.opt
local vwo = vim.wo

vg.mapleader = ' '
vg.maplocalleader = ' '

for _, provider in ipairs { 'node', 'perl', 'python3', 'ruby' } do
  vg['loaded_' .. provider .. '_provider'] = 0
end

vo.autoindent = true
vo.backspace = 'indent,eol,start'
vo.breakindent = true
vo.colorcolumn = '120'
vo.completeopt = 'menuone,noselect'
vo.cursorline = true
vo.expandtab = true
vo.hlsearch = true
vo.ignorecase = true
vo.list = true
vo.mouse = 'a'
vo.number = true
vo.shiftwidth = 2
vo.sidescroll = 1
vo.signcolumn = 'yes'
vo.smartcase = true
vo.splitbelow = true
vo.splitright = true
vo.tabstop = 2
vo.termguicolors = true
vo.undofile = true
vo.updatetime = 250
vo.wrap = false
vopt.clipboard:append('unnamedplus')
vopt.iskeyword:append('-')
vopt.shortmess:append('sI')
vwo.number = true
vwo.signcolumn = 'yes'

vim.keymap.set('', 'gn', ':bn<CR>')
vim.keymap.set('', 'gp', ':bp<CR>')
vim.keymap.set('', 'ga', '<C-^>')
vim.keymap.set({ 'n', 'v' }, 'gh', '0')
vim.keymap.set({ 'n', 'v' }, 'gl', '$')
vim.keymap.set({ 'n', 'v' }, 'gs', '_')
vim.keymap.set({ 'n', 'v' }, 'mm', '%', { remap = true })

vim.keymap.set('v', 'p', '"_dP')
vim.keymap.set('v', '>', '>gv')
vim.keymap.set('v', '<', '<gv')
vim.keymap.set('n', '>', '>>')
vim.keymap.set('n', '<', '<<')
vim.keymap.set({ 'n', 'v' }, 'U', '<C-r>')

vim.keymap.set('n', '<C-u>', '<C-u>zz')
vim.keymap.set('n', '<C-d>', '<C-d>zz')
vim.keymap.set('n', '<C-o>', '<C-o>zz')
vim.keymap.set('n', '<C-i>', '<C-i>zz')
vim.keymap.set('n', '<C-j>', '<C-Down>', { remap = true })
vim.keymap.set('n', '<C-k>', '<C-Up>', { remap = true })

vim.keymap.set({ 'n', 'v' }, '<leader><leader>', ':w!<CR>')
vim.keymap.set({ 'n', 'v' }, '<leader>x', ':bd<CR>')
vim.keymap.set({ 'n', 'v' }, '<leader>X', ':bd!<CR>')
vim.keymap.set({ 'n', 'v' }, '<leader>w', ':wa<CR>')
vim.keymap.set({ 'n', 'v' }, '<leader>W', ':wa!<CR>')
vim.keymap.set({ 'n', 'v' }, '<leader>q', ':q<CR>')
vim.keymap.set({ 'n', 'v' }, '<leader>Q', ':q!<CR>')
vim.keymap.set('n', '<esc>', ':noh<CR>')

vim.keymap.set('n', 'dp', vim.diagnostic.goto_prev)
vim.keymap.set('n', 'dn', vim.diagnostic.goto_next)
vim.keymap.set('n', '<leader>e', vim.diagnostic.open_float)

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

vim.api.nvim_create_autocmd('TextYankPost', {
  group = vim.api.nvim_create_augroup('YankHighlight', { clear = true }),
  pattern = '*',
  callback = function() vim.highlight.on_yank() end,
})
