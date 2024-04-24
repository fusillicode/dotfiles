return {
  'andrewferrier/debugprint.nvim',
  keys = { 'g?', },
  dependencies = {
    'nvim-treesitter/nvim-treesitter',
  },
  opts = {
    display_counter = false,
    display_snippet = false,
    move_to_debugline = true,
    print_tag = 'FOO',
  },
}
