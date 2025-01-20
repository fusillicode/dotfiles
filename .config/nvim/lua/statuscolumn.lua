local M = {}

M.draw_statuscolumn = require('rua').draw_statuscolumn

function M.draw(cur_lnum)
  local line_signs = vim.api.nvim_buf_get_extmarks(
    vim.fn.bufnr(), -1, { cur_lnum - 1, 0, }, { cur_lnum - 1, -1, },
    { type = 'sign', details = true, overlap = false, }
  )
  return M.draw_statuscolumn(cur_lnum, line_signs)
end

return M
