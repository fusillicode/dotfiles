return {
  'mizlan/delimited.nvim',
  opts = {
    post = function()
      local delimited = require('delimited')
      local keymap_set = require('utils').keymap_set

      keymap_set('n', 'dp', delimited.goto_prev)
      keymap_set('n', 'dn', delimited.goto_next)
    end,
  },
}
