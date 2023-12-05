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
vim.o.showmode = false
vim.o.number = true
vim.o.shiftwidth = 2
vim.o.sidescroll = 1
vim.o.signcolumn = 'auto:2'
vim.o.smartcase = true
vim.o.splitbelow = true
vim.o.splitright = true
vim.o.tabstop = 2
vim.o.termguicolors = true
vim.o.undofile = true
vim.o.updatetime = 250
vim.o.wrap = false

vim.opt.clipboard:append('unnamedplus')
vim.opt.iskeyword:append('-')
vim.opt.shortmess:append('sI')

vim.keymap.set('', 'gn', ':bn<CR>')
vim.keymap.set('', 'gp', ':bp<CR>')
vim.keymap.set('', 'ga', '<C-^>')
vim.keymap.set({ 'n', 'v', }, 'gh', '0')
vim.keymap.set({ 'n', 'v', }, 'gl', '$')
vim.keymap.set({ 'n', 'v', }, 'gs', '_')
vim.keymap.set({ 'n', 'v', }, 'mm', '%', { remap = true, })

vim.keymap.set('v', 'p', '"_dP')
vim.keymap.set('v', '>', '>gv')
vim.keymap.set('v', '<', '<gv')
vim.keymap.set('n', '>', '>>')
vim.keymap.set('n', '<', '<<')
vim.keymap.set({ 'n', 'v', }, 'U', '<C-r>')

vim.keymap.set('n', '<C-u>', '<C-u>zz')
vim.keymap.set('n', '<C-d>', '<C-d>zz')
vim.keymap.set('n', '<C-o>', '<C-o>zz')
vim.keymap.set('n', '<C-i>', '<C-i>zz')
vim.keymap.set('n', '<C-j>', '<C-Down>', { remap = true, })
vim.keymap.set('n', '<C-k>', '<C-Up>', { remap = true, })

vim.keymap.set({ 'n', 'v', }, '<leader><leader>', ':w!<CR>')
vim.keymap.set({ 'n', 'v', }, '<leader>x', ':bd<CR>')
vim.keymap.set({ 'n', 'v', }, '<leader>X', ':bd!<CR>')
vim.keymap.set({ 'n', 'v', }, '<leader>w', ':wa<CR>')
vim.keymap.set({ 'n', 'v', }, '<leader>W', ':wa!<CR>')
vim.keymap.set({ 'n', 'v', }, '<leader>q', ':q<CR>')
vim.keymap.set({ 'n', 'v', }, '<leader>Q', ':q!<CR>')
vim.keymap.set('n', '<esc>', ':noh<CR>')

vim.keymap.set('n', 'dp', vim.diagnostic.goto_prev)
vim.keymap.set('n', 'dn', vim.diagnostic.goto_next)
vim.keymap.set('n', '<leader>e', vim.diagnostic.open_float)
vim.diagnostic.config {
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
