local M = {}

local set_hl = vim.api.nvim_set_hl
local get_hl = vim.api.nvim_get_hl

function M.setup(colorscheme)
  if colorscheme then vim.cmd.colorscheme(colorscheme) end

  vim.o.background = 'dark'
  vim.o.termguicolors = true
  vim.o.colorcolumn = '120'

  local status_line_hl = { fg = 'gray', bg = 'none', }

  for hl, value in pairs({
    ColorColumn = { bg = 'NvimDarkGrey3', },
    CursorLine = { fg = 'none', },
    StatusLine = status_line_hl,
    MsgArea = status_line_hl,
    TelescopePreviewTitle = status_line_hl,
  }) do set_hl(0, hl, value) end

  for _, lvl in ipairs({ 'Error', 'Warn', 'Info', 'Hint', 'Ok', }) do
    local diagn_hl = get_hl(0, { name = 'Diagnostic' .. lvl, })
    diagn_hl.bg = status_line_hl.bg
    set_hl(0, 'DiagnosticStatusLine' .. lvl, diagn_hl)
  end
end

return M
