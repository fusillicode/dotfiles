return {
  'projekt0n/github-nvim-theme',
  priority = 1000,
  config = function()
    require('github-theme').setup({
      options = {
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

    vim.cmd.colorscheme('github_dark_dimmed')
  end,
}
