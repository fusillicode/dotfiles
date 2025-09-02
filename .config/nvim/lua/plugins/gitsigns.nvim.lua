local keymaps = require('keymaps')
local plugin_keymaps = keymaps.gitsigns

return {
  'lewis6991/gitsigns.nvim',
  event = 'BufReadPost',
  keys = plugin_keymaps(),
  opts = {
    on_attach = function(_)
      keymaps.set(plugin_keymaps(package.loaded.gitsigns))
    end,
  },
}
