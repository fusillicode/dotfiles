return {
  'neovim/nvim-lspconfig',
  event = { 'BufReadPre', 'BufNewFile', },
  dependencies = {
    'hrsh7th/cmp-nvim-lsp',
    'williamboman/mason-lspconfig.nvim',
    'williamboman/mason.nvim',
  },
  config = function()
    local lspconfig = require('lspconfig')
    local capabilities = require('cmp_nvim_lsp').default_capabilities(vim.lsp.protocol.make_client_capabilities())

    local function on_attach(_, bufnr)
      vim.keymap.set('', '<C-r>', ':LspRestart<CR>')
      vim.keymap.set('n', 'K', vim.lsp.buf.hover, { buffer = bufnr, })
      vim.keymap.set('n', '<leader>r', vim.lsp.buf.rename, { buffer = bufnr, })
      vim.keymap.set('n', '<leader>a', vim.lsp.buf.code_action, { buffer = bufnr, })
    end

    for lsp, config in pairs(require('../mason-tools')['lsps']) do
      lspconfig[lsp].setup({
        capabilities = capabilities,
        on_attach = on_attach,
        settings = config['settings'] or {},
        init_options = config['init_options'] or {},
      })
    end

    vim.api.nvim_create_autocmd('BufWritePre', {
      group = vim.api.nvim_create_augroup('LspFormatOnSave', { clear = true, }),
      callback = function() vim.lsp.buf.format({ async = false, }) end,
    })
  end,
}
