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

local function region_to_text(region)
  local text = ''
  local maxcol = vim.v.maxcol
  for line, cols in vim.spairs(region) do
    local endcol = cols[2] == maxcol and -1 or cols[2]
    local chunk = vim.api.nvim_buf_get_text(0, line, cols[1], line, endcol, {})[1]
    text = ('%s%s\n'):format(text, chunk)
  end
  return text
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

function M.open_selection()
  open(region_to_text(vim.region(0, "'<", "'>", vim.fn.visualmode(), true)))
end

return M
