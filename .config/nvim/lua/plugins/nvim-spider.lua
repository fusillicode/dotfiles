return {
  'chrisgrieser/nvim-spider',
  keys = {
    { 'w', mode = { 'n', 'o', 'x', }, },
    { 'e', mode = { 'n', 'o', 'x', }, },
    { 'b', mode = { 'n', 'o', 'x', }, },
  },
  config = function()
    require('keymaps').nvim_spider()
  end,
}
