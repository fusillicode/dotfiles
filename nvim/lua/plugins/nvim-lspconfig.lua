return {
  'neovim/nvim-lspconfig',
  event = { 'BufReadPre', 'BufNewFile', },
  dependencies = {
    'hrsh7th/cmp-nvim-lsp',
    'williamboman/mason-lspconfig.nvim',
    'williamboman/mason.nvim',
  },
  config = function()
    local mason_lspconfig = require('mason-lspconfig')
    local lsp_servers = require('../lsp-servers')
    mason_lspconfig.setup({ ensure_installed = vim.tbl_keys(lsp_servers), })

    local capabilities = vim.lsp.protocol.make_client_capabilities()
    capabilities = require('cmp_nvim_lsp').default_capabilities(capabilities)

    local function lsp_keybindings(_, bufnr)
      vim.keymap.set('', '<C-r>', ':LspRestart<CR>')
      vim.keymap.set('n', 'K', vim.lsp.buf.hover, { buffer = bufnr, })
      vim.keymap.set('n', '<leader>r', vim.lsp.buf.rename, { buffer = bufnr, })
      vim.keymap.set('n', '<leader>a', vim.lsp.buf.code_action, { buffer = bufnr, })
    end

    local lspconfig = require('lspconfig')

    mason_lspconfig.setup_handlers {
      function(server_name)
        lspconfig[server_name].setup({
          capabilities = capabilities,
          on_attach = lsp_keybindings,
          settings = lsp_servers[server_name],
        })
      end,
    }

    vim.lsp.handlers['textDocument/hover'] = vim.lsp.with(vim.lsp.handlers.hover, { border = 'single', })

    vim.api.nvim_create_autocmd('BufWritePre', {
      group = vim.api.nvim_create_augroup('LspFormatOnSave', { clear = true, }),
      callback = function() vim.lsp.buf.format({ async = false, }) end,
    })
  end,
}
