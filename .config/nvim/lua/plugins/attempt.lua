return {
  'm-demare/attempt.nvim',
  keys = { '<leader>n', },
  dependencies = { 'nvim-lua/plenary.nvim', 'nvim-telescope/telescope-ui-select.nvim', },
  config = function()
    local attempt = require('attempt')
    require('keymaps').attempt(attempt)

    attempt.setup({
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
  end,
}
