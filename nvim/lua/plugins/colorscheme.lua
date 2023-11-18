return {
  'catppuccin/nvim',
  event = 'VeryLazy',
  name = 'catppuccin',
  config = function()
    require('catppuccin').setup({
      dim_inactive = { enabled = true },
      no_italic = true,
      styles = {
        comments = {},
        conditionals = { 'bold' },
        loops = { 'bold' },
        functions = { 'bold' },
        keywords = { 'bold' },
        types = { 'bold' },
      },
      custom_highlights = function(colors)
        return {
          CursorLineNr = { fg = 'white', bold = true },
          GitSignsAdd = { fg = 'limegreen' },
          GitSignsChange = { fg = 'orange' },
          GitSignsDelete = { fg = 'red' },
          LineNr = { fg = 'grey' },
          LspInlayHint = { fg = 'grey', bg = colors.none },
          MatchParen = { fg = 'black', bg = 'orange' },
        }
      end
    })

    vim.cmd.colorscheme('catppuccin')
  end,
}
