local keymaps = require('keymaps')
local plugin_keymaps = keymaps.opencode

return {
  'NickvanDyke/opencode.nvim',
  keys = plugin_keymaps(),
  config = function()
    local plugin = require('opencode')
    plugin.setup({})
    keymaps.set(plugin_keymaps(plugin))
  end,
}
