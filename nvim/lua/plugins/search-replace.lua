return {
  'roobert/search-replace.nvim',
  keys = { '<leader>', mode = { 'n', 'v', }, },
  config = function()
    require('keymaps').search_replace()
    require('search-replace').setup({})
  end,
}
