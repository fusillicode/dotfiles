local M = {}

M.draw_statusline = require('rua').draw_statusline

function M.draw()
  -- Thank you ChatGPT
  local current_buffer_path = vim.fn.fnamemodify(
    vim.api.nvim_buf_get_name(vim.api.nvim_win_get_buf(vim.fn.win_getid(vim.fn.winnr('#')))),
    ':~:.'
  )
  return M.draw_statusline(vim.fn.bufnr(), current_buffer_path, vim.diagnostic.get())
end

vim.api.nvim_create_autocmd({ 'DiagnosticChanged', 'BufEnter', }, {
  group = vim.api.nvim_create_augroup('StatusLine', {}),
  callback = function() vim.o.statusline = M.draw() end,
})

return M
