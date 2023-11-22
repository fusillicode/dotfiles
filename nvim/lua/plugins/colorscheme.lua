return {
  'EdenEast/nightfox.nvim',
  priority = 1000,
  config = function()
    require('nightfox').setup({
      options = {
        styles = {
          conditionals = 'bold',
          functions = 'bold',
          keywords = 'bold',
          types = 'bold',
        },
        color_blind = {
          enable = true,
        },
      },
      groups = {
        all = {
          ColorColumn           = { bg = '#242830', },
          CursorLine            = { bg = '#242830', },
          CursorLineNr          = { fg = 'white', style = 'bold', },
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
