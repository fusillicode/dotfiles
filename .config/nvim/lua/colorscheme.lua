local M = {}

function M.setup(colorscheme)
  if colorscheme then vim.cmd.colorscheme(colorscheme) end

  vim.o.background = 'dark'
  vim.o.termguicolors = true
  vim.o.colorcolumn = '120'

  local status_line_hl = { bg = 'none', }
  for hl, value in pairs({
    ColorColumn = { bg = 'NvimDarkGrey3', },
    StatusLine = status_line_hl,
  }) do vim.api.nvim_set_hl(0, hl, value) end

  for _, diagnostic_hl_group in ipairs({
    'DiagnosticError',
    'DiagnosticWarn',
    'DiagnosticInfo',
    'DiagnosticHint',
    'DiagnosticOk',
  }) do
    vim.api.nvim_set_hl(0, diagnostic_hl_group .. 'StatusLine',
      { fg = vim.api.nvim_get_hl(0, { name = diagnostic_hl_group, }).fg, bg = status_line_hl.bg, }
    )
  end
end

return M
