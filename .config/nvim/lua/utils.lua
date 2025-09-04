local M = {}

function M.dbg(debug_value)
  print(vim.inspect(debug_value))
  return debug_value
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
