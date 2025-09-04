local M = {}

local statusline = require('nvrim').statusline

vim.api.nvim_create_autocmd({ 'DiagnosticChanged', 'BufEnter', }, {
  group = vim.api.nvim_create_augroup('StatusLine', {}),
  callback = function() vim.o.statusline = statusline.draw(vim.diagnostic.get()) end,
})

return M
