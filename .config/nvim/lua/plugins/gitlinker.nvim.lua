local keymaps = require('keymaps')
local plugin_keymaps = keymaps.gitlinker

return {
  'linrongbin16/gitlinker.nvim',
  keys = plugin_keymaps(),
  dependencies = { 'nvim-lua/plenary.nvim', },
  config = function()
    local plugin = require('gitlinker')
    plugin.setup({ message = false, })
    keymaps.set(plugin_keymaps(plugin))
  end,
}
