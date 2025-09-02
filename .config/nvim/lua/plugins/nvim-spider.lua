local keymaps = require('keymaps')
local plugin_keymaps = keymaps.nvim_spider

return {
  'chrisgrieser/nvim-spider',
  keys = plugin_keymaps(),
  config = function()
    local plugin = require('spider')
    plugin.setup({})
    keymaps.set(plugin_keymaps(plugin))
  end,
}
