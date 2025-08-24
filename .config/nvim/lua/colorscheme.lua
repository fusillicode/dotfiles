local M = {}

local set_hl = vim.api.nvim_set_hl
local get_hl = vim.api.nvim_get_hl

function M.setup(colorscheme)
  if colorscheme then vim.cmd.colorscheme(colorscheme) end

  vim.o.background = 'dark'
  vim.o.termguicolors = true

  local status_line_hl = { fg = 'gray', bg = 'none', }

  for hl, value in pairs({
    ColorColumn = { bg = 'NvimDarkGrey3', },
    CursorLine = { fg = 'none', },
    MsgArea = status_line_hl,
    Normal = { bg = '#000000', },
    StatusLine = status_line_hl,
  }) do set_hl(0, hl, value) end

  for _, lvl in ipairs({ 'Error', 'Warn', 'Info', 'Hint', 'Ok', }) do
    local diagn_hl = get_hl(0, { name = 'Diagnostic' .. lvl, })
    diagn_hl.bg = status_line_hl.bg
    set_hl(0, 'DiagnosticStatusLine' .. lvl, diagn_hl)

    local diagn_underline_hl = get_hl(0, { name = 'DiagnosticUnderline' .. lvl, })
    diagn_underline_hl.undercurl = true
    set_hl(0, 'DiagnosticUnderline' .. lvl, diagn_underline_hl)
  end
end

M.window = { border = 'rounded', }

return M
