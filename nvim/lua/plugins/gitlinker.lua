return {
  'linrongbin16/gitlinker.nvim',
  keys = { '<leader>', mode = { 'n', 'v', }, },
  dependencies = { 'nvim-lua/plenary.nvim', },
  config = function()
    local keymap_set = require('utils').keymap_set
    keymap_set({ 'n', 'v', }, '<leader>yl', ':GitLink<cr>')
    keymap_set({ 'n', 'v', }, '<leader>yL', ':GitLink!<cr>')
    keymap_set({ 'n', 'v', }, '<leader>yb', ':GitLink blame<cr>')
    keymap_set({ 'n', 'v', }, '<leader>yB', ':GitLink! blame<cr>')

    require('gitlinker').setup({
      message = false,
    })
  end,
}
