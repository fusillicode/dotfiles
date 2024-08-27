return {
  'jake-stewart/multicursor.nvim',
  keys = {
    { '<c-j>', mode = { 'n', }, },
    { '<c-k>', mode = { 'n', }, },
    { '<c-n>', mode = { 'n', }, },
  },
  config = function()
    local mc = require('multicursor-nvim')
    mc.setup()

    vim.cmd.hi('link', 'MultiCursorCursor', 'Cursor')
    vim.cmd.hi('link', 'MultiCursorVisual', 'Visual')

    require('keymaps').multicursor(mc)
  end,
}
