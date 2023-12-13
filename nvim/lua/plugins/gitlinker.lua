return {
  'linrongbin16/gitlinker.nvim',
  keys = {
    { 'yg', mode = { 'n', 'v', }, },
    { 'yG', mode = { 'n', 'v', }, },
    { 'yb', mode = { 'n', 'v', }, },
    { 'yB', mode = { 'n', 'v', }, },
  },
  dependencies = { 'nvim-lua/plenary.nvim', },
  config = function()
    local keymap_set = require('utils').keymap_set
    keymap_set({ 'n', 'v', }, 'yg', '<cmd>GitLink<cr>')
    keymap_set({ 'n', 'v', }, 'yG', '<cmd>GitLink!<cr>')
    keymap_set({ 'n', 'v', }, 'yb', '<cmd>GitLink blame<cr>')
    keymap_set({ 'n', 'v', }, 'yB', '<cmd>GitLink! blame<cr>')

    require('gitlinker').setup()
  end,
}
