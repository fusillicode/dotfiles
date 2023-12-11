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
vim.o.statuscolumn = '%{%v:lua.require("statuscol").draw(bufnr(), v:lnum)%}'

function statusline()
  local buffer = vim.fn.bufnr()
  local buffer_errors, buffer_warns, buffer_hints, buffer_infos = 0, 0, 0, 0
  local workspace_errors, workspace_warns, workspace_hints, workspace_infos = 0, 0, 0, 0

  for _, diagnostic in ipairs(vim.diagnostic.get()) do
    if diagnostic.bufnr == buffer then
      if diagnostic.severity == vim.diagnostic.severity.ERROR then
        buffer_errors = buffer_errors + 1
      elseif diagnostic.severity == vim.diagnostic.severity.WARN then
        buffer_warns = buffer_warns + 1
      elseif diagnostic.severity == vim.diagnostic.severity.HINT then
        buffer_hints = buffer_hints + 1
      elseif diagnostic.severity == vim.diagnostic.severity.INFO then
        buffer_infos = buffer_infos + 1
      end
    end
    if diagnostic.severity == vim.diagnostic.severity.ERROR then
      workspace_errors = workspace_errors + 1
    elseif diagnostic.severity == vim.diagnostic.severity.WARN then
      workspace_warns = workspace_warns + 1
    elseif diagnostic.severity == vim.diagnostic.severity.HINT then
      workspace_hints = workspace_hints + 1
    elseif diagnostic.severity == vim.diagnostic.severity.INFO then
      workspace_infos = workspace_infos + 1
    end
  end

  return (buffer_errors ~= 0 and '%#DiagnosticError#' .. 'E:' .. buffer_errors .. ' ' or '')
      .. (buffer_warns ~= 0 and '%#DiagnosticWarn#' .. 'W:' .. buffer_warns .. ' ' or '')
      .. (buffer_hints ~= 0 and '%#DiagnosticHint#' .. 'H:' .. buffer_hints .. ' ' or '')
      .. (buffer_infos ~= 0 and '%#DiagnosticInfo#' .. 'I:' .. buffer_infos .. ' ' or '')
      .. '%#StatusLine#'
      .. '%f %m %r'
      .. '%='
      .. (workspace_errors ~= 0 and '%#DiagnosticError#' .. 'E:' .. workspace_errors .. ' ' or '')
      .. (workspace_warns ~= 0 and '%#DiagnosticWarn#' .. 'W:' .. workspace_warns .. ' ' or '')
      .. (workspace_hints ~= 0 and '%#DiagnosticHint#' .. 'H:' .. workspace_hints .. ' ' or '')
      .. (workspace_infos ~= 0 and '%#DiagnosticInfo#' .. 'I:' .. workspace_infos .. ' ' or '')
end

vim.o.statusline = '%{%v:lua.statusline()%}'

vim.api.nvim_create_autocmd({ 'DiagnosticChanged', 'BufEnter', }, {
  group = vim.api.nvim_create_augroup('StatusLine', { clear = true, }),
  callback = function(_) vim.o.statusline = statusline() end,
})

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
keymap_set('', 'gn', ':bn<CR>')
keymap_set('', 'gp', ':bp<CR>')
keymap_set('', 'ga', '<C-^>')
keymap_set({ 'n', 'v', }, 'gh', '0')
keymap_set({ 'n', 'v', }, 'gl', '$')
keymap_set({ 'n', 'v', }, 'gs', '_')
keymap_set({ 'n', 'v', }, 'mm', '%', { remap = true, })

keymap_set('v', 'p', '"_dP')
keymap_set('v', '>', '>gv')
keymap_set('v', '<', '<gv')
keymap_set('n', '>', '>>')
keymap_set('n', '<', '<<')
keymap_set({ 'n', 'v', }, 'U', '<C-r>')

keymap_set('n', '<C-u>', '<C-u>zz')
keymap_set('n', '<C-d>', '<C-d>zz')
keymap_set('n', '<C-o>', '<C-o>zz')
keymap_set('n', '<C-i>', '<C-i>zz')
keymap_set('n', '<C-j>', '<C-Down>', { remap = true, })
keymap_set('n', '<C-k>', '<C-Up>', { remap = true, })

keymap_set({ 'n', 'v', }, '<leader><leader>', ':silent :w!<CR>')
keymap_set({ 'n', 'v', }, '<leader>x', ':bd<CR>')
keymap_set({ 'n', 'v', }, '<leader>X', ':bd!<CR>')
keymap_set({ 'n', 'v', }, '<leader>w', ':wa<CR>')
keymap_set({ 'n', 'v', }, '<leader>W', ':wa!<CR>')
keymap_set({ 'n', 'v', }, '<leader>q', ':q<CR>')
keymap_set({ 'n', 'v', }, '<leader>Q', ':q!<CR>')
keymap_set({ 'n', 'v', }, '<C-e>', ':luafile %<CR>')
keymap_set('n', '<esc>', ':noh<CR>', { silent = false, })

keymap_set('i', "'", "''<left>")
keymap_set('i', '"', '""<left>')
keymap_set('i', '(', '()<left>')
keymap_set('i', '[', '[]<left>')
keymap_set('i', '{', '{}<left>')
keymap_set('i', '<', '<><left>')
keymap_set('i', '{;', '{};<left><left>')

keymap_set('n', '<C-s>"', 'ciw""<esc>P')
keymap_set('n', '<C-s>(', 'ciw()<esc>P')
keymap_set('n', '<C-s>[', 'ciw[]<esc>P')
keymap_set('n', '<C-s>{', 'ciw{}<esc>P')
keymap_set('n', "<C-s>'", "ciw''<esc>P")
keymap_set('n', '<C-s><', 'ciw<><esc>P')

keymap_set('v', '<C-s>"', 'c""<esc>P')
keymap_set('v', '<C-s>(', 'c()<esc>P')
keymap_set('v', '<C-s>[', 'c[]<esc>P')
keymap_set('v', '<C-s>{', 'c{}<esc>P')
keymap_set('v', "<C-s>'", "c''<esc>P")
keymap_set('v', '<C-s><', 'c<><esc>P')

keymap_set('n', 'dp', vim.diagnostic.goto_prev)
keymap_set('n', 'dn', vim.diagnostic.goto_next)
keymap_set('n', '<leader>e', vim.diagnostic.open_float)

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
