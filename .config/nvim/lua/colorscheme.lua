vim.o.colorcolumn = '120'
vim.o.termguicolors = true

local hint = 'cyan'
local ok = 'limegreen'
local warn = 'orange'
local err = 'red'
local note = 'fuchsia'

local match_highlight = { fg = vim.api.nvim_get_hl(0, { name = 'Special', }).fg, bold = true, }
for hl, value in pairs({
  CmpItemMenu = { fg = vim.api.nvim_get_hl(0, { name = 'IncSearch', }).bg, italic = true, },
  ColorColumn = { link = 'CursorLine', },
  StatusLine = { link = 'CursorLine', },
  DiagnosticError = { fg = err, },
  DiagnosticWarn = { fg = warn, },
  DiagnosticInfo = { fg = hint, },
  DiagnosticHint = { fg = note, },
  DiagnosticOk = { fg = ok, },
  DiagnosticFloatingError = { fg = err, },
  DiagnosticFloatingWarn = { fg = warn, },
  DiagnosticFloatingInfo = { fg = hint, },
  DiagnosticFloatingHint = { fg = note, },
  DiagnosticFloatingOk = { fg = ok, },
  DiagnosticSignError = { fg = err, },
  DiagnosticSignWarn = { fg = warn, },
  DiagnosticSignInfo = { fg = hint, },
  DiagnosticSignHint = { fg = note, },
  DiagnosticSignOk = { fg = ok, },
  DiagnosticUnderlineError = { undercurl = true, sp = err, },
  DiagnosticUnderlineWarn = { undercurl = true, sp = warn, },
  DiagnosticUnderlineInfo = { undercurl = true, sp = hint, },
  DiagnosticUnderlineHint = { undercurl = true, sp = note, },
  DiagnosticUnderlineOk = { undercurl = true, sp = ok, },
  DiagnosticVirtualTextError = { fg = err, },
  DiagnosticVirtualTextWarn = { fg = warn, },
  DiagnosticVirtualTextInfo = { fg = hint, },
  DiagnosticVirtualTextHint = { fg = note, },
  DiagnosticVirtualTextOk = { fg = ok, },
  DiagnosticUnnecessary = { fg = '', undercurl = true, sp = warn, },
  DiagnosticDeprecated = { fg = '', strikethrough = true, },
  GitSignsAddInline = { link = 'GitSignsAdd', },
  GitSignsChangeInline = { link = 'GitSignsChange', },
  GitSignsDeleteInline = { link = 'GitSignsDelete', },
  GitSignsAdd = { fg = ok, },
  GitSignsAddPreview = { fg = ok, },
  GitSignsChange = { fg = warn, },
  GitSignsChangePreview = { fg = warn, },
  GitSignsDelete = { fg = err, },
  GitSignsDeletePreview = { fg = err, },
  GrugFarResultsLineColumn = { link = 'LineNr', },
  GrugFarResultsLineNo = { link = 'LineNr', },
  GrugFarResultsMatch = match_highlight,
  GrugFarResultsPath = { link = 'ModeMsg', },
  GrugFarResultsStats = match_highlight,
  LspInlayHint = { link = 'LineNr', },
  TelescopeMatching = match_highlight,
  TelescopePromptCounter = match_highlight,
  TelescopePromptPrefix = match_highlight,
  TelescopeResultsDiffAdd = { fg = ok, },
  TelescopeResultsDiffChange = { fg = warn, },
  TelescopeResultsDiffDelete = { fg = err, },
  TelescopeResultsDiffUntracked = { fg = hint, },
}) do vim.api.nvim_set_hl(0, hl, value) end

local status_line_hl = vim.api.nvim_get_hl(0, { name = 'CursorLine', })
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
