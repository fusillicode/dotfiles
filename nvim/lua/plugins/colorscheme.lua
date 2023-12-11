return {
  'EdenEast/nightfox.nvim',
  priority = 1000,
  config = function()
    local nordfox_light_bg = '#242830'

    require('nightfox').setup({
      options = {
        color_blind = {
          enable = true,
        },
      },
      groups = {
        all = {
          ColorColumn = { bg = nordfox_light_bg, },
          CursorLine = { bg = nordfox_light_bg, },
          CursorLineNr = { fg = 'white', style = 'bold', },
          DiagnosticError = { fg = 'red', },
          DiagnosticWarn = { fg = 'orange', },
          DiagnosticInfo = { fg = 'magenta', },
          DiagnosticHint = { fg = 'cyan', },
          DiagnosticOk = { fg = 'limegreen', },
          DiagnosticFloatingError = { fg = 'red', bg = nordfox_light_bg, },
          DiagnosticFloatingWarn = { fg = 'orange', bg = nordfox_light_bg, },
          DiagnosticFloatingInfo = { fg = 'magenta', bg = nordfox_light_bg, },
          DiagnosticFloatingHint = { fg = 'cyan', bg = nordfox_light_bg, },
          DiagnosticFloatingOk = { fg = 'limegreen', bg = nordfox_light_bg, },
          DiagnosticUnderlineError = { sp = 'red', },
          DiagnosticUnderlineWarn = { sp = 'orange', },
          DiagnosticUnderlineInfo = { sp = 'magenta', },
          DiagnosticUnderlineHint = { sp = 'cyan', },
          DiagnosticUnderlineOk = { sp = 'limegreen', },
          GitSignsAdd = { fg = 'limegreen', },
          GitSignsChange = { fg = 'orange', },
          GitSignsDelete = { fg = 'red', },
          LspInlayHint = { fg = 'grey', bg = 'none', },
          MatchParen = { fg = 'black', bg = 'orange', },
          TelescopeMatching = { fg = 'orange', },
          TelescopePromptPrefix = { link = 'TelescopePromptBorder', },
        },
      },
      palettes = {
        all = {
          bg1 = '#171b21',
        },
      },
    })

    vim.cmd.colorscheme('nordfox')
  end,
}
