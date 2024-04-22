return {
  'andrewferrier/debugprint.nvim',
  event = 'BufReadPost',
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
