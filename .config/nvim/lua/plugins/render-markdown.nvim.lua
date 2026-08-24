return {
  'MeanderingProgrammer/render-markdown.nvim',
  ft = { 'markdown', },
  dependencies = { 'nvim-treesitter/nvim-treesitter', },
  opts = {},
  config = function()
    require('render-markdown').setup({
      anti_conceal = {
        enabled = false,
      },
      completions = {
        blink = { enabled = true, },
        lsp = { enabled = true, },
      },
    })
  end,
}
