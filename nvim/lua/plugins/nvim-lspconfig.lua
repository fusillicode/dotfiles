return {
  'neovim/nvim-lspconfig',
  event = { 'BufReadPost', 'BufNewFile' },
  dependencies = {
    'williamboman/mason.nvim',
    'williamboman/mason-lspconfig.nvim',
  },
  config = function()
    local lsp_servers = {
      bashls = {},
      docker_compose_language_service = {},
      dockerls = {},
      dotls = {},
      graphql = {},
      html = {},
      helm_ls = {},
      jsonls = {},
      lua_ls = {
        Lua = {
          diagnostics = { globals = { 'vim' } },
          telemetry = { enable = false },
          workspace = { checkThirdParty = false }
        },
      },
      marksman = {},
      ruff_lsp = {},
      rust_analyzer = {
        ['rust-analyzer'] = {
          cargo = {
            build_script = { enable = true },
            extraArgs = { '--profile', 'rust-analyzer' },
            extraEnv = { CARGO_PROFILE_RUST_ANALYZER_INHERITS = 'dev' },
          },
          check = { command = 'clippy' },
          checkOnSave = { command = 'clippy' },
          completion = { autoimport = { enable = true } },
          imports = { enforce = true, granularity = { group = 'item' }, prefix = 'crate' },
          lens = { debug = { enable = false }, implementations = { enable = false }, run = { enable = false } },
          proc_macro = { enable = true },
          showUnlinkedFileNotification = false
        }
      },
      sqlls = {},
      taplo = {},
      tsserver = {},
      yamlls = {}
    }

    require('mason').setup({})

    local mason_lspconfig = require('mason-lspconfig')
    mason_lspconfig.setup({ ensure_installed = vim.tbl_keys(lsp_servers) })

    local capabilities = vim.lsp.protocol.make_client_capabilities()
    capabilities = require('cmp_nvim_lsp').default_capabilities(capabilities)

    local lsp_keybindings = function(_, bufnr)
      vim.keymap.set('', '<C-r>', ':LspRestart<CR>')
      vim.keymap.set('n', 'K', vim.lsp.buf.hover, { buffer = bufnr })
      vim.keymap.set('n', '<leader>r', vim.lsp.buf.rename, { buffer = bufnr })
      vim.keymap.set('n', '<leader>a', vim.lsp.buf.code_action, { buffer = bufnr })
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

    vim.lsp.handlers['textDocument/hover'] = vim.lsp.with(vim.lsp.handlers.hover, { border = 'single' })

    vim.api.nvim_create_autocmd('BufWritePre', {
      group = vim.api.nvim_create_augroup('LspFormatOnSave', {}),
      callback = function() vim.lsp.buf.format({ async = false }) end,
    })
  end
}
