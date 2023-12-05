return {
  'EdenEast/nightfox.nvim',
  priority = 1000,
  config = function()
    require('nightfox').setup({
      options = {
        color_blind = {
          enable = true,
        },
      },
      groups = {
        all = {
          ColorColumn           = { bg = '#242830', },
          CursorLine            = { bg = '#242830', },
          CursorLineNr          = { fg = 'white', style = 'bold', },
          DiagnosticError       = { fg = 'red', },
          DiagnosticHint        = { fg = 'aqua', },
          DiagnosticInfo        = { fg = 'white', },
          DiagnosticOk          = { fg = 'limegreen', },
          DiagnosticWarn        = { fg = 'orange', },
          GitSignsAdd           = { fg = 'limegreen', },
          GitSignsChange        = { fg = 'orange', },
          GitSignsDelete        = { fg = 'red', },
          LspInlayHint          = { fg = 'grey', bg = 'none', },
          MatchParen            = { fg = 'black', bg = 'orange', },
          TelescopeMatching     = { fg = 'orange', },
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
