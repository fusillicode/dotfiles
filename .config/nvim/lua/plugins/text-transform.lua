return {
  'chenasraf/text-transform.nvim',
  keys = { { '<leader>u', mode = { 'n', 'v', }, }, },
  config = function()
    require('text-transform').setup({
      popup_type = 'select',
    })
  end,
}
