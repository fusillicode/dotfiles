vim.loader.enable()

require('commands')
require('keymaps').core()

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
vim.o.shortmess = 'asIF'
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

local function trim_trailing_dot(str)
  return string.gsub(str, '%.(?=\r?\n)', '')
end

vim.diagnostic.config {
  float = {
    anchor_bias = 'above',
    border = 'rounded',
    focusable = true,
    format = function(diagnostic)
      local src_code = {}
      if diagnostic.source then
        table.insert(src_code, (trim_trailing_dot(diagnostic.source)))
      end
      if diagnostic.code then
        table.insert(src_code, (trim_trailing_dot(diagnostic.code)))
      end

      return '▶ '
          .. trim_trailing_dot(string.gsub(diagnostic.message, '[\n\r]', ', '))
          .. (next(src_code) == nil and '' or ' [' .. table.concat(src_code, ': ') .. ']')
    end,
    header = '',
    prefix = '',
    source = false,
    suffix = '',
  },
  severity_sort = true,
  signs = true,
  underline = true,
  update_in_insert = true,
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

