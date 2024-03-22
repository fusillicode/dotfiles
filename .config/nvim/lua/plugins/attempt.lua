return {
  'm-demare/attempt.nvim',
  dependencies = { 'nvim-lua/plenary.nvim', },
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
