return {
  'mcchrish/zenbones.nvim',
  priority = 1000,
  config = function()
    vim.g.bones_compat = 1
    vim.cmd.colorscheme('neobones')

    local match_highlight = { fg = vim.api.nvim_get_hl(0, { name = 'IncSearch', }).bg, bold = true, }

    for hl, value in pairs({
      CursorLine = { bg = vim.api.nvim_get_hl(0, { name = 'StatusLine', }).bg, },
      ColorColumn = { link = 'StatusLine', },
      DiagnosticError = { fg = 'red', },
      DiagnosticWarn = { fg = 'orange', },
      DiagnosticInfo = { fg = 'cyan', },
      DiagnosticHint = { fg = 'white', },
      DiagnosticOk = { fg = 'limegreen', },
      DiagnosticFloatingError = { fg = 'red', },
      DiagnosticFloatingWarn = { fg = 'orange', },
      DiagnosticFloatingInfo = { fg = 'cyan', },
      DiagnosticFloatingHint = { fg = 'white', },
      DiagnosticFloatingOk = { fg = 'limegreen', },
      DiagnosticSignError = { fg = 'red', },
      DiagnosticSignWarn = { fg = 'orange', },
      DiagnosticSignInfo = { fg = 'cyan', },
      DiagnosticSignHint = { fg = 'white', },
      DiagnosticSignOk = { fg = 'limegreen', },
      DiagnosticUnderlineError = { undercurl = true, sp = 'red', },
      DiagnosticUnderlineWarn = { undercurl = true, sp = 'orange', },
      DiagnosticUnderlineInfo = { undercurl = true, sp = 'cyan', },
      DiagnosticUnderlineHint = { undercurl = true, sp = 'white', },
      DiagnosticUnderlineOk = { undercurl = true, sp = 'limegreen', },
      DiagnosticVirtualTextError = { fg = 'red', },
      DiagnosticVirtualTextWarn = { fg = 'orange', },
      DiagnosticVirtualTextInfo = { fg = 'cyan', },
      DiagnosticVirtualTextHint = { fg = 'white', },
      DiagnosticVirtualTextOk = { fg = 'limegreen', },
      DiagnosticUnnecessary = { fg = '', undercurl = true, sp = 'orange', },
      DiagnosticDeprecated = { fg = '', strikethrough = true, },
      GitSignsAdd = { fg = 'limegreen', },
      GitSignsChange = { fg = 'orange', },
      GitSignsDelete = { fg = 'red', },
      TelescopeMatching = match_highlight,
      TelescopePromptCounter = match_highlight,
      TelescopePromptPrefix = { link = 'TelescopePromptBorder', },
      TelescopeResultsDiffAdd = { fg = 'limegreen', },
      TelescopeResultsDiffChange = { fg = 'orange', },
      TelescopeResultsDiffDelete = { fg = 'red', },
      TelescopeResultsDiffUntracked = { fg = 'cyan', },
    }) do
      vim.api.nvim_set_hl(0, hl, value)
    end
  end,
}
