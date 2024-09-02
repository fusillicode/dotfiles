return {
  'chenasraf/text-transform.nvim',
  dependencies = { 'nvim-telescope/telescope.nvim', },
  keys = { { '<leader>u', mode = { 'n', 'v', }, }, },
  config = function()
    require('text-transform').setup({
      keymap = {
        telescope_popup = {
          ['n'] = '<leader>u',
          ['v'] = '<leader>u',
        },
      },
    })
  end,
}
