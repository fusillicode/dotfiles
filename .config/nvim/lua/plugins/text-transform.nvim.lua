local keymaps = require('keymaps')
local plugin_keymaps = keymaps.text_transform

return {
  'chenasraf/text-transform.nvim',
  keys = plugin_keymaps(),
  config = function()
    local plugin = require('text-transform')
    plugin.setup({
      popup_type = 'select',
      keymap = {
        telescope_popup = {
          ['n'] = '<leader>u',
          ['v'] = '<leader>u',
        },
      },
    })
  end,
}
