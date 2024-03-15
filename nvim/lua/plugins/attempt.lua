return {
  'm-demare/attempt.nvim',
  dependencies = { 'nvim-lua/plenary.nvim', },
  config = function()
    local attempt = require('attempt')
    local keymap_set = require('utils').keymap_set

    keymap_set('n', '<leader>n', attempt.new_input_ext)

    attempt.setup({
      autosave = true,
      list_buffers = true,
      initial_content = {
        sh = '#!/usr/bin/env bash\n\nset -euo pipefail',
        json = '{}',
        md = '# foo',
        rs = 'fn main() {\n  println!("Hello, world!");\n}',
      },
      ext_options = { 'sh', 'json', 'md', 'rs', },
    })
  end,
}
