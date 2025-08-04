local enabled_file_types = { 'markdown', 'copilot-chat', }

return {
  'MeanderingProgrammer/render-markdown.nvim',
  ft           = enabled_file_types,
  dependencies = { 'nvim-treesitter/nvim-treesitter', },
  configure    = function()
    require('render-markdown').setup({
      file_types = enabled_file_types,
      completions = {
        blink = { enabled = true, },
        lsp = { enabled = true, },
      },
    })
  end,
}
