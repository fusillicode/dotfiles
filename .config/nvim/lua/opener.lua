local M = {}

local function open(s)
  local thing = vim.fn.expand(vim.fn.trim(s))
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

function M.open_under_cursor()
  local cursor_col = vim.api.nvim_win_get_cursor(0)[2]
  local line = vim.api.nvim_get_current_line()

  local before = line:sub(1, cursor_col)
  local after = line:sub(cursor_col + 1, #line)

  local _, _, before_match = before:find('(%S+)$')
  local _, _, after_match = after:find('^(%S+)')

  open((before_match or '') .. (after_match or ''))
end

-- https://github.com/neovim/neovim/pull/13896
function M.open_selection()
  local _, ls, cs = table.unpack(vim.fn.getpos('v'))
  local _, le, ce = table.unpack(vim.fn.getpos('.'))
  open(vim.api.nvim_buf_get_text(0, ls - 1, cs - 1, le - 1, ce, {}))
end

return M
