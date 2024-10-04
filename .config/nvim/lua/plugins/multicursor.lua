return {
  'jake-stewart/multicursor.nvim',
  keys = {
    { '<c-j>', mode = { 'n', 'v', }, },
    { '<c-k>', mode = { 'n', 'v', }, },
    { '<c-n>', mode = { 'n', 'v', }, },
    { '<c-p>', mode = { 'n', 'v', }, },
  },
  config = function()
    local mc = require('multicursor-nvim')
    mc.setup()

    require('keymaps').multicursor(mc)
  end,
}
