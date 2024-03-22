return {
  'linrongbin16/gitlinker.nvim',
  keys = {
    { '<leader>yl', mode = { 'n', 'v', }, },
    { '<leader>yL', mode = { 'n', 'v', }, },
    { '<leader>yb', mode = { 'n', 'v', }, },
    { '<leader>yB', mode = { 'n', 'v', }, },
  },
  dependencies = { 'nvim-lua/plenary.nvim', },
  config = function()
    require('keymaps').gitlinker()
    require('gitlinker').setup({
      message = false,
    })
  end,
}
