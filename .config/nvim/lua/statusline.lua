-- Thank you ChatGPT
local function current_buffer_path()
  return vim.fn.fnamemodify(
    vim.api.nvim_buf_get_name(vim.api.nvim_win_get_buf(vim.fn.win_getid(vim.fn.winnr('#')))),
    ':~:.'
  )
end

local M = {}

function M.draw()
  return require('rua').draw_statusline(vim.fn.bufnr(), current_buffer_path(), vim.diagnostic.get())
end

vim.api.nvim_create_autocmd({ 'DiagnosticChanged', 'BufEnter', }, {
  group = vim.api.nvim_create_augroup('StatusLine', {}),
  callback = function() vim.o.statusline = M.draw() end,
})

return M
