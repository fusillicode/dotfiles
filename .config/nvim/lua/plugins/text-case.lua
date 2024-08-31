return {
  'johmsalas/text-case.nvim',
  dependencies = { 'nvim-telescope/telescope.nvim', },
  config = function()
    local textcase = require('textcase')
    textcase.setup({})

    require('telescope').load_extension('textcase')
    require('keymaps').textcase(textcase)
  end,
}
