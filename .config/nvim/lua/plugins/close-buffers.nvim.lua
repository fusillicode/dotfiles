local keymaps = require('keymaps')
local plugin_keymaps = keymaps.close_buffers

return {
  'kazhala/close-buffers.nvim',
  keys = plugin_keymaps(),
  config = function()
    local plugin = require('close_buffers')
    plugin.setup({})
    keymaps.set(plugin_keymaps(plugin))
  end,
}
