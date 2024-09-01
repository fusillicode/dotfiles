return {
  'johmsalas/text-case.nvim',
  dependencies = { 'nvim-telescope/telescope.nvim', },
  keys = '<leader>u',
  config = function()
    require('textcase').setup({})
    require('telescope').load_extension('textcase')
    require('keymaps').textcase()
  end,
}
