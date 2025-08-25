local M = {}

M.draw_statusline = require('rua2').draw_statusline

function M.draw() return M.draw_statusline(vim.diagnostic.get()) end

vim.api.nvim_create_autocmd({ 'DiagnosticChanged', 'BufEnter', }, {
  group = vim.api.nvim_create_augroup('StatusLine', {}),
  callback = function() vim.o.statusline = M.draw() end,
})

return M
