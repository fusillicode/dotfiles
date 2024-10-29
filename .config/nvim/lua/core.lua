vim.loader.enable()

require('commands')
require('keymaps').core()
require('colorscheme').setup()

for _, provider in ipairs { 'node', 'perl', 'python3', 'ruby', } do
  vim.g['loaded_' .. provider .. '_provider'] = 0
end

vim.o.autoindent = true
vim.o.backspace = 'indent,eol,start'
vim.o.breakindent = true
vim.o.completeopt = 'menuone,noselect'
vim.o.cursorline = true
vim.o.expandtab = true
vim.o.hlsearch = true
vim.o.ignorecase = true
vim.o.laststatus = 3
vim.o.list = true
vim.o.mouse = 'a'
vim.o.number = true
vim.o.shiftwidth = 2
vim.o.shortmess = 'ascIF'
vim.o.showmode = false
vim.o.showtabline = 0
vim.o.sidescroll = 1
vim.o.signcolumn = 'no'
vim.o.smartcase = true
vim.o.splitbelow = true
vim.o.splitright = true
vim.o.statuscolumn = '%{%v:lua.require("statuscolumn").draw(v:lnum)%}'
vim.o.statusline = '%{%v:lua.require("statusline").draw()%}'
vim.o.swapfile = false
vim.o.tabstop = 2
vim.o.undofile = true
vim.o.updatetime = 250
vim.o.wrap = false

vim.opt.clipboard:append('unnamedplus')
vim.opt.iskeyword:append('-')
vim.opt.jumpoptions:append('stack')

vim.diagnostic.config {
  float = {
    anchor_bias = 'above',
    border = 'rounded',
    focusable = true,
    format = function(diagnostic)
      local message =
          vim.tbl_get(diagnostic, 'user_data', 'lsp', 'data', 'rendered') or
          vim.tbl_get(diagnostic, 'user_data', 'lsp', 'message')
      local source = vim.tbl_get(diagnostic, 'user_data', 'lsp', 'source'):gsub('%.$', '')
      local code = vim.tbl_get(diagnostic, 'user_data', 'lsp', 'code'):gsub('%.$', '')
      local from = vim.tbl_get(diagnostic, 'user_data', 'lsp', 'range', 'start')
      local to = vim.tbl_get(diagnostic, 'user_data', 'lsp', 'range', 'end')

      return '▶ ' ..
          message:gsub('%.$', '') ..
          ' [' .. source .. ': ' .. code ..
          ' @ ' .. from.line .. ':' .. from.character .. ';' .. to.line .. ':' .. to.character .. ']'
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

vim.api.nvim_create_autocmd({ 'BufLeave', 'FocusLost', }, {
  group = vim.api.nvim_create_augroup('AutosaveBuffers', { clear = true, }),
  command = ':silent! wa!',
})

vim.api.nvim_create_autocmd({ 'FileType', }, {
  group = vim.api.nvim_create_augroup('QuickfixConfig', { clear = true, }),
  pattern = 'qf',
  callback = function() require('keymaps').quickfix() end,
})
