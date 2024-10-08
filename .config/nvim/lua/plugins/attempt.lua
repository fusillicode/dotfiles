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
        sh = '#!/usr/bin/env bash\n\nset -euo pipefail',
        json = '{}',
        md = '# README.md',
        rs = 'fn main() {\n  println!("Hello, world!");\n}',
      },
      ext_options = { 'sh', 'json', 'md', 'rs', },
    })
  end,
}
