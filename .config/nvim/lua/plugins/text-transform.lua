return {
  'chenasraf/text-transform.nvim',
  keys = { { '<leader>u', mode = { 'n', 'v', }, }, },
  config = function()
    require('text-transform').setup({
      popup_type = 'select',
      keymap = {
        telescope_popup = {
          ['n'] = '<leader>u',
          ['v'] = '<leader>u',
        },
      },
    })
  end,
}
