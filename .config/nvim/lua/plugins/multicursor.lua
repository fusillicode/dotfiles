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

    vim.keymap.set('n', '<esc>', function()
      if mc.hasCursors() then mc.clearCursors() end
      vim.api.nvim_command('noh | echo""')
    end)

    vim.keymap.set('n', '<c-j>', function() mc.addCursor('j') end)
    vim.keymap.set('n', '<c-k>', function() mc.addCursor('k') end)
    vim.keymap.set('n', '<c-n>', function() mc.addCursor('*') end)
  end,
}
