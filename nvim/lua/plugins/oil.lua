return {
  'stevearc/oil.nvim',
  keys = {
    { '<leader>F', },
  },
  config = function()
    vim.keymap.set('n', '<leader>F', '<CMD>Oil --float<CR>')

    require('oil').setup({
      delete_to_trash = true,
      view_options = {
        show_hidden = true,
      },
      float = {
        padding = 2,
        max_width = 100,
        max_height = 30,
        override = function(conf)
          return vim.tbl_extend('force', conf, { anchor = 'SW', })
        end,
      },
      keymaps = {
        ['<esc>'] = ':bd!<CR>',
      },
    })
  end,
}