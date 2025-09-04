local M = {}

M.statusline = require('nvrim').statusline

function M.draw()
  return M.statusline.draw(vim.diagnostic.get())
end

vim.api.nvim_create_autocmd({ 'DiagnosticChanged', 'BufEnter', }, {
  group = vim.api.nvim_create_augroup('StatusLine', {}),
  callback = function() vim.o.statusline = M.draw() end,
})

return M
