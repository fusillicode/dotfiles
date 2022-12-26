require('telescope').setup{
  defaults = {
    layout_strategy = 'vertical',
  },
  pickers = {
    find_files = {
      find_command = {'rg', '--ignore', '-L', '--hidden', '--files'},
    }
  }
}

local builtin = require('telescope.builtin')

vim.keymap.set('n', '<leader>ff', builtin.find_files, {})
vim.keymap.set('n', '<leader>fg', builtin.live_grep, {})
vim.keymap.set('n', '<leader>fb', builtin.buffers, {})

