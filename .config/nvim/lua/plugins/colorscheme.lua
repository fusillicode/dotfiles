return {
  'mcchrish/zenbones.nvim',
  priority = 1000,
  config = function()
    vim.g.bones_compat = 1
    vim.cmd.colorscheme('neobones')

    local blue = 'cyan'
    local green = 'limegreen'
    local orange = 'orange'
    local red = 'red'
    local white = 'white'

    local match_highlight = { fg = vim.api.nvim_get_hl(0, { name = 'IncSearch', }).bg, bold = true, }
    local telescope_border = { fg = vim.api.nvim_get_hl(0, { name = 'LineNr', }).fg, bold = true, }

    for hl, value in pairs({
      CmpItemMenu = { fg = vim.api.nvim_get_hl(0, { name = 'IncSearch', }).bg, italic = true, },
      CursorLine = { bg = vim.api.nvim_get_hl(0, { name = 'StatusLine', }).bg, },
      ColorColumn = { link = 'StatusLine', },
      DiagnosticError = { fg = red, },
      DiagnosticWarn = { fg = orange, },
      DiagnosticInfo = { fg = blue, },
      DiagnosticHint = { fg = white, },
      DiagnosticOk = { fg = green, },
      DiagnosticFloatingError = { fg = red, },
      DiagnosticFloatingWarn = { fg = orange, },
      DiagnosticFloatingInfo = { fg = blue, },
      DiagnosticFloatingHint = { fg = white, },
      DiagnosticFloatingOk = { fg = green, },
      DiagnosticSignError = { fg = red, },
      DiagnosticSignWarn = { fg = orange, },
      DiagnosticSignInfo = { fg = blue, },
      DiagnosticSignHint = { fg = white, },
      DiagnosticSignOk = { fg = green, },
      DiagnosticUnderlineError = { undercurl = true, sp = red, },
      DiagnosticUnderlineWarn = { undercurl = true, sp = orange, },
      DiagnosticUnderlineInfo = { undercurl = true, sp = blue, },
      DiagnosticUnderlineHint = { undercurl = true, sp = white, },
      DiagnosticUnderlineOk = { undercurl = true, sp = green, },
      DiagnosticVirtualTextError = { fg = red, },
      DiagnosticVirtualTextWarn = { fg = orange, },
      DiagnosticVirtualTextInfo = { fg = blue, },
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
      TelescopePreviewBorder = telescope_border,
      TelescopePromptBorder = telescope_border,
      TelescopeResultsBorder = telescope_border,
      TelescopeResultsDiffAdd = { fg = green, },
      TelescopeResultsDiffChange = { fg = orange, },
      TelescopeResultsDiffDelete = { fg = red, },
      TelescopeResultsDiffUntracked = { fg = blue, },
    }) do
      vim.api.nvim_set_hl(0, hl, value)
    end
  end,
}
