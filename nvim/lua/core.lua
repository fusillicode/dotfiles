local vg = vim.g
vg.mapleader = ' '
vg.maplocalleader = ' '

for _, provider in ipairs { 'node', 'perl', 'python3', 'ruby', } do
  vg['loaded_' .. provider .. '_provider'] = 0
end

local vo = vim.o
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
vo.showmode = false
vo.number = true
vo.shiftwidth = 2
vo.sidescroll = 1
vo.signcolumn = 'auto:2'
vo.smartcase = true
vo.splitbelow = true
vo.splitright = true
vo.tabstop = 2
vo.termguicolors = true
vo.undofile = true
vo.updatetime = 250
vo.wrap = false

local vopt = vim.opt
vopt.clipboard:append('unnamedplus')
vopt.iskeyword:append('-')
vopt.shortmess:append('sI')

local vkeyset = vim.keymap.set
vkeyset('', 'gn', ':bn<CR>')
vkeyset('', 'gp', ':bp<CR>')
vkeyset('', 'ga', '<C-^>')
vkeyset({ 'n', 'v', }, 'gh', '0')
vkeyset({ 'n', 'v', }, 'gl', '$')
vkeyset({ 'n', 'v', }, 'gs', '_')
vkeyset({ 'n', 'v', }, 'mm', '%', { remap = true, })

vkeyset('v', 'p', '"_dP')
vkeyset('v', '>', '>gv')
vkeyset('v', '<', '<gv')
vkeyset('n', '>', '>>')
vkeyset('n', '<', '<<')
vkeyset({ 'n', 'v', }, 'U', '<C-r>')

vkeyset('n', '<C-u>', '<C-u>zz')
vkeyset('n', '<C-d>', '<C-d>zz')
vkeyset('n', '<C-o>', '<C-o>zz')
vkeyset('n', '<C-i>', '<C-i>zz')
vkeyset('n', '<C-j>', '<C-Down>', { remap = true, })
vkeyset('n', '<C-k>', '<C-Up>', { remap = true, })

vkeyset({ 'n', 'v', }, '<leader><leader>', ':w!<CR>')
vkeyset({ 'n', 'v', }, '<leader>x', ':bd<CR>')
vkeyset({ 'n', 'v', }, '<leader>X', ':bd!<CR>')
vkeyset({ 'n', 'v', }, '<leader>w', ':wa<CR>')
vkeyset({ 'n', 'v', }, '<leader>W', ':wa!<CR>')
vkeyset({ 'n', 'v', }, '<leader>q', ':q<CR>')
vkeyset({ 'n', 'v', }, '<leader>Q', ':q!<CR>')
vkeyset('n', '<esc>', ':noh<CR>')

local vdiag = vim.diagnostic
vkeyset('n', 'dp', vdiag.goto_prev)
vkeyset('n', 'dn', vdiag.goto_next)
vkeyset('n', '<leader>e', vdiag.open_float)
vdiag.config {
  float = {
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
  underline = false,
  update_in_insert = false,
  virtual_text = false,
}

vim.api.nvim_create_autocmd('TextYankPost', {
  group = vim.api.nvim_create_augroup('YankHighlight', { clear = true, }),
  pattern = '*',
  callback = function() vim.highlight.on_yank() end,
})

vim.api.nvim_create_autocmd('FocusLost', {
  group = vim.api.nvim_create_augroup('AutosaveBuffer', { clear = true, }),
  command = ':silent! wa!',
})
