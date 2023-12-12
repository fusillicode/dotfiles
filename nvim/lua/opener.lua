local M = {}

function M.open_under_cursor()
  local cursor_col = vim.api.nvim_win_get_cursor(0)[2]
  local line = vim.api.nvim_get_current_line()

  local before = line:sub(1, cursor_col)
  local after = line:sub(cursor_col + 1, #line)

  local _, _, before_match = before:find('(%S+)$')
  local _, _, after_match = after:find('^(%S+)')

  local thing = vim.fn.expand(vim.fn.trim((before_match or '') .. (after_match or '')))
  if thing == '' then return end
  vim.fn.jobstart({ 'open', thing, }, {
    detach = true,
    on_exit = function(_, code, _)
      if code ~= 0 then
        print('Cannot open "' .. thing .. '" 🤖')
      end
    end,
  })
end

return M
