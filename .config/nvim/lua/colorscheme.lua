vim.o.colorcolumn = '120'
vim.o.termguicolors = true

local cyan = 'cyan'
local green = 'limegreen'
local orange = 'orange'
local red = 'red'
local white = 'white'

local match_highlight = { fg = vim.api.nvim_get_hl(0, { name = 'Special', }).fg, bold = true, }
for hl, value in pairs({
  CmpItemMenu = { fg = vim.api.nvim_get_hl(0, { name = 'IncSearch', }).bg, italic = true, },
  ColorColumn = { link = 'CursorLine', },
  StatusLine = { link = 'CursorLine', },
  DiagnosticError = { fg = red, },
  DiagnosticWarn = { fg = orange, },
  DiagnosticInfo = { fg = cyan, },
  DiagnosticHint = { fg = white, },
  DiagnosticOk = { fg = green, },
  DiagnosticFloatingError = { fg = red, },
  DiagnosticFloatingWarn = { fg = orange, },
  DiagnosticFloatingInfo = { fg = cyan, },
  DiagnosticFloatingHint = { fg = white, },
  DiagnosticFloatingOk = { fg = green, },
  DiagnosticSignError = { fg = red, },
  DiagnosticSignWarn = { fg = orange, },
  DiagnosticSignInfo = { fg = cyan, },
  DiagnosticSignHint = { fg = white, },
  DiagnosticSignOk = { fg = green, },
  DiagnosticUnderlineError = { undercurl = true, sp = red, },
  DiagnosticUnderlineWarn = { undercurl = true, sp = orange, },
  DiagnosticUnderlineInfo = { undercurl = true, sp = cyan, },
  DiagnosticUnderlineHint = { undercurl = true, sp = white, },
  DiagnosticUnderlineOk = { undercurl = true, sp = green, },
  DiagnosticVirtualTextError = { fg = red, },
  DiagnosticVirtualTextWarn = { fg = orange, },
  DiagnosticVirtualTextInfo = { fg = cyan, },
  DiagnosticVirtualTextHint = { fg = white, },
  DiagnosticVirtualTextOk = { fg = green, },
  DiagnosticUnnecessary = { fg = '', undercurl = true, sp = orange, },
  DiagnosticDeprecated = { fg = '', strikethrough = true, },
  GitSignsAddInline = { link = 'GitSignsAdd', },
  GitSignsChangeInline = { link = 'GitSignsChange', },
  GitSignsDeleteInline = { link = 'GitSignsDelete', },
  GitSignsAdd = { fg = green, },
  GitSignsAddPreview = { fg = green, },
  GitSignsChange = { fg = orange, },
  GitSignsChangePreview = { fg = orange, },
  GitSignsDelete = { fg = red, },
  GitSignsDeletePreview = { fg = red, },
  GrugFarResultsLineColumn = { link = 'LineNr', },
  GrugFarResultsLineNo = { link = 'LineNr', },
  GrugFarResultsMatch = match_highlight,
  GrugFarResultsPath = { link = 'Comment', },
  GrugFarResultsStats = match_highlight,
  LspInlayHint = { link = 'LineNr', },
  TelescopeMatching = match_highlight,
  TelescopePromptCounter = match_highlight,
  TelescopePromptPrefix = match_highlight,
  TelescopeResultsDiffAdd = { fg = green, },
  TelescopeResultsDiffChange = { fg = orange, },
  TelescopeResultsDiffDelete = { fg = red, },
  TelescopeResultsDiffUntracked = { fg = cyan, },
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
