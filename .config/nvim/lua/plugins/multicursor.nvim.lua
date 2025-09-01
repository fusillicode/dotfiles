local keymaps = require('keymaps')
local plugin_keymaps = keymaps.multicursor

return {
  'jake-stewart/multicursor.nvim',
  keys = plugin_keymaps(),
  config = function()
    local plugin = require('multicursor-nvim')

    plugin.setup({})
    keymaps.set(plugin_keymaps(plugin))
  end,
}
