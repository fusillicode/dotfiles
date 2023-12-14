return {
  'linrongbin16/gitlinker.nvim',
  keys = { { '<leader>', mode = { 'n', 'v', }, }, },
  dependencies = { 'nvim-lua/plenary.nvim', },
  config = function()
    local keymap_set = require('utils').keymap_set
    keymap_set({ 'n', 'v', }, '<leader>yl', '<cmd>GitLink<cr>')
    keymap_set({ 'n', 'v', }, '<leader>yL', '<cmd>GitLink!<cr>')
    keymap_set({ 'n', 'v', }, '<leader>yb', '<cmd>GitLink blame<cr>')
    keymap_set({ 'n', 'v', }, '<leader>yB', '<cmd>GitLink! blame<cr>')

    require('gitlinker').setup()
  end,
}
