local enabled_file_types = { 'markdown', 'copilot-chat', }

return {
  'MeanderingProgrammer/render-markdown.nvim',
  ft           = enabled_file_types,
  dependencies = { 'nvim-treesitter/nvim-treesitter', },
  config       = function()
    require('render-markdown').setup({
      file_types = enabled_file_types,
      anti_conceal = {
        enabled = false,
      },
      render_modes = true,
      completions = {
        blink = { enabled = true, },
        lsp = { enabled = true, },
      },
      code = {
        language = false,
      },
    })
  end,
}
