local M = {}

function M.draw(current_lnum)
  local line_signs = vim.api.nvim_buf_get_extmarks(
    vim.fn.bufnr(), -1, { current_lnum - 1, 0, }, { current_lnum - 1, -1, },
    { type = 'sign', details = true, overlap = false, }
  )
  return require('rua').draw_statuscolumn(line_signs) .. ' %=%{v:lnum} '
end

return M
