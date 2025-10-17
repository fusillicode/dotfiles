local keymaps = require('keymaps')
local style_opts = require('nvrim').style_opts
local plugin_keymaps = keymaps.oil

return {
  'stevearc/oil.nvim',
  keys = plugin_keymaps(),
  config = function()
    local plugin = require('oil')
    plugin.setup({
      buf_options = {
        buflisted = false,
        bufhidden = 'hide',
      },
      delete_to_trash = true,
      float = {
        border = style_opts['window']['border'],
        max_width = 100,
        max_height = 30,
        override = function(conf)
          return vim.tbl_extend('error', conf, { anchor = 'SW', })
        end,
      },
      confirmation = style_opts['window']['border'],
      progress = style_opts['window']['border'],
      ssh = style_opts['window']['border'],
      keymaps_help = style_opts['window']['border'],
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
    keymaps.set(plugin_keymaps(plugin))
  end,
}
