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
          ColorColumn              = { bg = nordfox_light_bg, },
          CursorLine               = { bg = nordfox_light_bg, },
          CursorLineNr             = { fg = 'white', style = 'bold', },
          DiagnosticError          = { fg = 'red', },
          DiagnosticWarn           = { fg = 'orange', },
          DiagnosticHint           = { fg = 'aqua', },
          DiagnosticInfo           = { fg = 'white', },
          DiagnosticOk             = { fg = 'limegreen', },
          DiagnosticFloatingError  = { fg = 'red', bg = nordfox_light_bg, },
          DiagnosticFloatingWarn   = { fg = 'orange', bg = nordfox_light_bg, },
          DiagnosticFloatingHint   = { fg = 'aqua', bg = nordfox_light_bg, },
          DiagnosticFloatingInfo   = { fg = 'white', bg = nordfox_light_bg, },
          DiagnosticFloatingOk     = { fg = 'limegreen', bg = nordfox_light_bg, },
          DiagnosticUnderlineError = { sp = 'red', },
          DiagnosticUnderlineWarn  = { sp = 'orange', },
          DiagnosticUnderlineHint  = { sp = 'aqua', },
          DiagnosticUnderlineInfo  = { sp = 'white', },
          DiagnosticUnderlineOk    = { sp = 'limegreen', },
          GitSignsAdd              = { fg = 'limegreen', },
          GitSignsChange           = { fg = 'orange', },
          GitSignsDelete           = { fg = 'red', },
          LspInlayHint             = { fg = 'grey', bg = 'none', },
          MatchParen               = { fg = 'black', bg = 'orange', },
          TelescopeMatching        = { fg = 'orange', },
          TelescopePromptPrefix    = { link = 'TelescopePromptBorder', },
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
