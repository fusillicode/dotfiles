return {
  'EdenEast/nightfox.nvim',
  priority = 1000,
  config = function()
    require('nightfox').setup({
      options = {
        colorblind = {
          enable = true,
        },
        styles = {
          conditionals = 'bold',
          functions = 'bold',
          keywords = 'bold',
          types = 'bold',
        },
      },
      groups = {
        all = {
          CursorLineNr      = { fg = 'white', style = 'bold' },
          GitSignsAdd       = { fg = 'limegreen' },
          GitSignsChange    = { fg = 'orange' },
          GitSignsDelete    = { fg = 'red' },
          LspInlayHint      = { fg = 'grey', bg = 'none' },
          MatchParen        = { fg = 'black', bg = 'orange' },
          TelescopeMatching = { fg = 'orange' }
        },
      },
    })

    vim.cmd.colorscheme('duskfox')
  end,
}
