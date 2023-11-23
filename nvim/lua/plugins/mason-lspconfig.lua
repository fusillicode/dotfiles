return {
  'williamboman/mason-lspconfig.nvim',
  dependencies = {
    'williamboman/mason.nvim',
  },
  lazy = true,
  config = function()
    require('mason-lspconfig').setup({
      ensure_installed = vim.tbl_keys(require('../lsp-servers')),
      automatic_installation = true,
    })
  end,
}
