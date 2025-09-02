local keymaps = require('keymaps')
local plugin_keymaps = keymaps.attempt

return {
  'm-demare/attempt.nvim',
  keys = plugin_keymaps(),
  dependencies = { 'nvim-lua/plenary.nvim', },
  config = function()
    local plugin = require('attempt')
    plugin.setup({
      autosave = true,
      list_buffers = true,
      initial_content = {
        json = '{}',
        md = '# README.md',
        rs = 'fn main() {\n  println!("Hello, world!");\n}',
        sh = '#!/usr/bin/env bash\n\nset -euo pipefail',
        sql = 'select * from where',
      },
      ext_options = {
        'json',
        'md',
        'rs',
        'sh',
        'sql',
      },
    })
    keymaps.set(plugin_keymaps(plugin))
  end,
}
