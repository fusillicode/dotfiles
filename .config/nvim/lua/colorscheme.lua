local M = {}

function M.setup(colorscheme)
  if colorscheme then vim.cmd.colorscheme(colorscheme) end

  vim.o.background = 'dark'
  vim.o.termguicolors = true
  vim.o.colorcolumn = '120'

  local set_hl = vim.api.nvim_set_hl
  local get_hl = vim.api.nvim_get_hl

  local status_line_hl = { bg = 'none', }
  for hl, value in pairs({
    ColorColumn = { bg = 'NvimDarkGrey3', },
    StatusLine = status_line_hl,
  }) do set_hl(0, hl, value) end

  for _, lvl in ipairs({ 'Error', 'Warn', 'Info', 'Hint', 'Ok', }) do
    set_hl(0, 'DiagnosticStatusLine' .. lvl,
      { fg = get_hl(0, { name = 'Diagnostic' .. lvl, }).fg, bg = status_line_hl.bg, }
    )
  end
end

return M
