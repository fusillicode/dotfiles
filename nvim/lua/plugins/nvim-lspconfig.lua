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
    local keymap_set = require('utils').keymap_set

    local function on_attach(_, bufnr)
      keymap_set('', '<C-r>', ':LspRestart<CR>')
      keymap_set('n', 'K', vim.lsp.buf.hover, { buffer = bufnr, })
      keymap_set('n', '<leader>r', vim.lsp.buf.rename, { buffer = bufnr, })
    end

    for lsp, config in pairs(require('mason-tools')['tools']['lsps']) do
      local lsp_setup = { capabilities = capabilities, on_attach = on_attach, }
      if config['cmd'] then lsp_setup.cmd = config['cmd'] end
      if config['settings'] then lsp_setup.settings = config['settings'] end
      if config['init_options'] then lsp_setup.init_options = config['init_options'] end
      lspconfig[lsp].setup(lsp_setup)
    end

    vim.api.nvim_create_autocmd('BufWritePre', {
      group = vim.api.nvim_create_augroup('LspFormatOnSave', { clear = true, }),
      callback = function() vim.lsp.buf.format({ async = false, }) end,
    })
  end,
}
