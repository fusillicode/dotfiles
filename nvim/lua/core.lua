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
vim.o.sidescroll = 1
vim.o.signcolumn = 'yes:1'
vim.o.smartcase = true
vim.o.splitbelow = true
vim.o.splitright = true
vim.o.tabstop = 2
vim.o.termguicolors = true
vim.o.undofile = true
vim.o.updatetime = 250
vim.o.wrap = true

-- function get_signs()
--   local buf = vim.api.nvim_win_get_buf(vim.g.statusline_winid)
--   return vim.tbl_map(function(sign)
--     return vim.fn.sign_getdefined(sign.name)[1]
--   end, vim.fn.sign_getplaced(buf, { group = '*', lnum = vim.v.lnum, })[1].signs)
-- end

local M = {}

function M.foo()
  require('utils').dbg(vim.l)
  if not vim then return end

  local row = vim.l.lnum
  local extmarks = vim.api.nvim_buf_get_extmarks(0, -1, { row - 1, 0, }, { row - 1, -1, },
    { type = 'sign', details = true, overlap = false, })

  print('--------')
  require('utils').dbg(extmarks)
  print('--------')

  local git_sign, diagnostics
  for _, sign in ipairs(extmarks) do
    if git_sign and diagnostics then break end
    local sign_details = sign[4]
    if sign_details.sign_hl_group:sub(1, #'GitSigns') == 'GitSigns' then
      git_sign = sign_details
    elseif sign_details.sign_hl_group:sub(1, #'Diagnostic') == 'Diagnostic' then
      diagnostics = sign_details
    end
  end
  local status_col = (
    '%{v:lnum}'
    .. (git_sign and ('%#' .. git_sign.sign_hl_group .. '#' .. git_sign.sign_text .. '%*') or ' ')
    .. (diagnostics and ('%#' .. diagnostics.sign_hl_group .. '#' .. diagnostics.sign_text .. '%*') or ' ')
  )

--  vim.o.statuscolumn = status_col
  return status_col
end

-- M.foo()

-- require('utils').dbg(vim.fn.sign_getdefined)

-- function column()
--   local sign, git_sign
--   for _, s in ipairs(get_signs()) do
--     if s.name:find('GitSign') then
--       git_sign = s
--     else
--       sign = s
--     end
--   end
--   local components = {
--     sign and ('%#' .. sign.texthl .. '#' .. sign.text .. '%*') or ' ',
--     [[%=]],
--     [[%{&nu?(&rnu&&v:relnum?v:relnum:v:lnum):''} ]],
--     git_sign and ('%#' .. git_sign.texthl .. '#' .. git_sign.text .. '%*') or '  ',
--   }
--   return table.concat(components, '')
-- end

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
keymap_set('n', '<esc>', ':noh<CR>', { silent = false, })

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
