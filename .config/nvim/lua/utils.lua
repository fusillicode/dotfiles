local M = {}

function M.dbg(debug_value)
  print(vim.inspect(debug_value))
  return debug_value
end

M.normal_esc = ':noh<cr>:echo""<cr>'

function M.visual_esc()
  return ":<c-u>'" .. (vim.fn.line('.') < vim.fn.line('v') and '<' or '>') .. '<cr>' .. M.normal_esc
end

function M.unpack(list)
  ---@diagnostic disable-next-line: deprecated
  return (table.unpack or unpack)(list)
end

-- https://github.com/nvim-telescope/telescope-live-grep-args.nvim/blob/731a046da7dd3adff9de871a42f9b7fb85f60f47/lua/telescope-live-grep-args/shortcuts.lua#L8-L17
function M.get_visual_selection_boundaries()
  local _, start_ln, start_col = M.unpack(vim.fn.getpos('v'))
  local _, end_ln, end_col = M.unpack(vim.fn.getpos('.'))

  start_ln, end_ln = math.min(start_ln, end_ln), math.max(start_ln, end_ln)
  start_col, end_col = math.min(start_col, end_col), math.max(start_col, end_col)

  return start_ln, start_col, end_ln, end_col
end

function M.get_visual_selection()
  local start_ln, start_col, end_ln, end_col = require('utils').get_visual_selection_boundaries()
  return vim.api.nvim_buf_get_text(0, start_ln - 1, start_col - 1, end_ln - 1, end_col, {})[1]
end

function M.escape_regex(str)
  return vim.fn.escape(str, [[\.^$*+?()[]{}|]])
end

function M.log(value)
  local log_path = vim.fn.stdpath('log') .. '/log'
  local file = io.open(log_path, 'a')
  if file then
    file:write(vim.inspect(value) .. '\n')
    file:close()
    return
  end
  vim.api.nvim_err_writeln('Failed to open log file ' .. log_path)
end

function M.item_idx(list, item)
  for idx, i in ipairs(list) do
    if i == item then return idx end
  end
end

return M
