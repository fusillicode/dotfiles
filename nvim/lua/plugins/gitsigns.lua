return {
  'lewis6991/gitsigns.nvim',
  event = 'BufReadPost',
  opts = {
    on_attach = function(_)
      require('keymaps').gitsigns(package.loaded.gitsigns)
    end,
  },
}
