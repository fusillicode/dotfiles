return {
  'stevearc/oil.nvim',
  keys = '<leader>F',
  config = function()
    require('keymaps').oil()

    require('oil').setup({
      buf_options = {
        buflisted = false,
        bufhidden = 'hide',
      },
      delete_to_trash = true,
      float = {
        padding = 2,
        max_width = 100,
        max_height = 30,
        override = function(conf)
          return vim.tbl_extend('force', conf, { anchor = 'SW', })
        end,
      },
      keymaps = {
        ['<esc>'] = ':bd!<cr>',
        ['<s-l>'] = 'actions.select',
        ['<s-h>'] = 'actions.parent',
      },
      prompt_save_on_select_new_entry = false,
      skip_confirm_for_simple_edits = true,
      view_options = {
        show_hidden = true,
      },
      experimental_watch_for_changes = true,
    })
  end,
}
