return {
  'mizlan/delimited.nvim',
  event = 'DiagnosticChanged',
  config = function()
    require('keymaps').delimited(require('delimited'))
  end,
}
