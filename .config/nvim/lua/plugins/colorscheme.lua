return {
  'yorickpeterse/nvim-grey',
  config = function()
    vim.cmd.colorscheme('Grey')

    vim.o.background = 'light'
    vim.o.termguicolors = true
    vim.o.colorcolumn = '120'

    vim.api.nvim_set_hl(0, 'Normal', { bg = 'white', })
    vim.api.nvim_set_hl(0, 'EndOfBuffer', { bg = 'white', fg = 'white', })
    vim.api.nvim_set_hl(0, 'Cursor', { bg = 'NvimLightGrey3', })
    vim.api.nvim_set_hl(0, 'MatchParen', { bg = 'NvimLightGrey2', })
    vim.api.nvim_set_hl(0, 'LspInlayHint', { fg = 'NvimLightGrey4', })

    for hl, value in pairs({
      ColorColumn = { link = 'CursorLine', },
      StatusLine = { link = 'CursorLine', },
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
        { fg = vim.api.nvim_get_hl(0, { name = diagnostic_hl_group, }).fg, bg = status_line_hl.bg, bold = true, }
      )
    end
  end,
}
