return {
  'mcchrish/zenbones.nvim',
  priority = 1000,
  config = function()
    vim.g.bones_compat = 1
    vim.cmd.colorscheme('rosebones')

    vim.api.nvim_set_hl(0, 'CursorLine', { bg = vim.api.nvim_get_hl(0, { name = 'StatusLine', }).bg, })
    vim.api.nvim_set_hl(0, 'ColorColumn', { link = 'StatusLine', })
    vim.api.nvim_set_hl(0, 'DiagnosticError', { fg = 'red', })
    vim.api.nvim_set_hl(0, 'DiagnosticWarn', { fg = 'orange', })
    vim.api.nvim_set_hl(0, 'DiagnosticInfo', { fg = 'cyan', })
    vim.api.nvim_set_hl(0, 'DiagnosticHint', { fg = 'white', })
    vim.api.nvim_set_hl(0, 'DiagnosticOk', { fg = 'limegreen', })
    vim.api.nvim_set_hl(0, 'DiagnosticFloatingError', { fg = 'red', })
    vim.api.nvim_set_hl(0, 'DiagnosticFloatingWarn', { fg = 'orange', })
    vim.api.nvim_set_hl(0, 'DiagnosticFloatingInfo', { fg = 'cyan', })
    vim.api.nvim_set_hl(0, 'DiagnosticFloatingHint', { fg = 'white', })
    vim.api.nvim_set_hl(0, 'DiagnosticFloatingOk', { fg = 'limegreen', })
    vim.api.nvim_set_hl(0, 'DiagnosticSignError', { fg = 'red', })
    vim.api.nvim_set_hl(0, 'DiagnosticSignWarn', { fg = 'orange', })
    vim.api.nvim_set_hl(0, 'DiagnosticSignInfo', { fg = 'cyan', })
    vim.api.nvim_set_hl(0, 'DiagnosticSignHint', { fg = 'white', })
    vim.api.nvim_set_hl(0, 'DiagnosticSignOk', { fg = 'limegreen', })
    vim.api.nvim_set_hl(0, 'DiagnosticUnderlineError', { undercurl = true, sp = 'red', })
    vim.api.nvim_set_hl(0, 'DiagnosticUnderlineWarn', { undercurl = true, sp = 'orange', })
    vim.api.nvim_set_hl(0, 'DiagnosticUnderlineInfo', { undercurl = true, sp = 'cyan', })
    vim.api.nvim_set_hl(0, 'DiagnosticUnderlineHint', { undercurl = true, sp = 'white', })
    vim.api.nvim_set_hl(0, 'DiagnosticUnderlineOk', { undercurl = true, sp = 'limegreen', })
    vim.api.nvim_set_hl(0, 'DiagnosticUnnecessary', { fg = '', undercurl = true, sp = 'orange', })
    vim.api.nvim_set_hl(0, 'DiagnosticDeprecated', { fg = '', strikethrough = true, })
    vim.api.nvim_set_hl(0, 'GitSignsAdd', { fg = 'limegreen', })
    vim.api.nvim_set_hl(0, 'GitSignsChange', { fg = 'orange', })
    vim.api.nvim_set_hl(0, 'GitSignsDelete', { fg = 'red', })
  end,
}
