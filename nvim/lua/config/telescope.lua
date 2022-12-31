require('telescope').setup {
  defaults = {
    layout_strategy = 'vertical',
  },
  pickers = {
    find_files = {
      find_command = { 'rg', '--ignore', '-L', '--hidden', '--files' },
    }
  }
}

local builtin = require('telescope.builtin')

vim.keymap.set('n', '<leader>f', builtin.find_files, {})
vim.keymap.set('n', '<leader>s', builtin.live_grep, {})
vim.keymap.set('n', '<leader>b', builtin.buffers, {})

require('telescope').load_extension('projects')

vim.keymap.set("n", "<leader>p", ":Telescope projects<cr>", {})
