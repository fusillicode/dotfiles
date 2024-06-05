local function get_lsps_configs()
  local home_dir    = os.getenv('HOME')
  local schemastore = require('schemastore')

  return {
    bashls = {},
    docker_compose_language_service = {},
    dockerls = {},
    elixirls = {
      cmd = { home_dir .. '/.local/bin/elixir-ls', },
      elixirLS = {
        signatureAfterComplete = true,
        suggestSpecs = true,
      },
    },
    elmls = {},
    graphql = {},
    html = {
      filetypes = { 'html', 'htmldjango', },
    },
    helm_ls = {},
    jsonls = {
      settings = {
        json = {
          validate = { enable = true, },
          schemas = schemastore.json.schemas({
            select = {
              'GitHub Workflow Template Properties',
            },
          }),
        },
      },
    },
    lua_ls = {
      cmd = { home_dir .. '/.dev-tools/lua-language-server/bin/lua-language-server', },
      settings = {
        Lua = {
          completion = {
            callSnippet = 'Both',
            callKeyword = 'Both',
          },
          format = {
            defaultConfig = {
              insert_final_newline = 'true',
              quote_style = 'single',
              trailing_table_separator = 'always',
            },
          },
          hint = { enable = true, setType = true, },
          diagnostics = { globals = { 'vim', }, },
          telemetry = { enable = false, },
          workspace = { checkThirdParty = false, },
        },
      },
    },
    marksman = {},
    phpactor = {},
    psalm = {
      root_dir = require('lspconfig.util').root_pattern({ 'psalm.xml', 'psalm.xml.dist', 'psalm-baseline.xml', }),
    },
    ruff_lsp = {},
    rust_analyzer = {
      settings = {
        ['rust-analyzer'] = {
          cargo = {
            build_script = { enable = true, },
            extraArgs = { '--profile', 'rust-analyzer', },
            extraEnv = { CARGO_PROFILE_RUST_ANALYZER_INHERITS = 'dev', },
          },
          check = { command = 'clippy', },
          checkOnSave = { command = 'clippy', },
          completion = { autoimport = { enable = true, }, },
          diagnostics = {
            enable = true,
            disabled = { 'unresolved-proc-macro', },
          },
          imports = { enforce = true, granularity = { group = 'item', }, prefix = 'crate', },
          lens = { debug = { enable = false, }, implementations = { enable = false, }, run = { enable = false, }, },
          procMacro = { enable = false, },
          showUnlinkedFileNotification = false,
        },
      },
    },
    sqlls = {},
    taplo = {},
    typos_lsp = {
      init_options = {
        diagnosticSeverity = 'Warning',
      },
    },
    tsserver = {
      init_options = {
        preferences = {
          includeInlayParameterNameHints = 'all',
          includeInlayParameterNameHintsWhenArgumentMatchesName = true,
          includeInlayFunctionParameterTypeHints = true,
          includeInlayVariableTypeHints = true,
          includeInlayPropertyDeclarationTypeHints = true,
          includeInlayFunctionLikeReturnTypeHints = true,
          includeInlayEnumMemberValueHints = true,
          importModuleSpecifierPreference = 'non-relative',
        },
      },
    },
    yamlls = {
      settings = {
        yaml = {
          schemaStore = {
            enable = false,
            url = '',
          },
          schemas = vim.tbl_extend('error',
            schemastore.yaml.schemas({
              select = {
                'kustomization.yaml',
                'GitHub Workflow',
                'docker-compose.yml',
              },
            }),
            { kubernetes = { 'k8s**.yaml', 'kube*/*.yaml', }, }
          ),
        },
      },
    },
  }
end

return {
  'neovim/nvim-lspconfig',
  event = 'BufRead',
  dependencies = {
    'hrsh7th/cmp-nvim-lsp',
    'b0o/schemastore.nvim',
  },
  config = function()
    local lspconfig = require('lspconfig')
    local capabilities = require('cmp_nvim_lsp').default_capabilities(vim.lsp.protocol.make_client_capabilities())
    local on_attach = require('keymaps').lspconfig()

    for lsp, config in pairs(get_lsps_configs()) do
      -- 🥲 https://neovim.discourse.group/t/cannot-serialize-function-type-not-supported/4542/3
      local lsp_setup = { capabilities = capabilities, on_attach = on_attach, }
      if config['cmd'] then lsp_setup.cmd = config['cmd'] end
      if config['filetypes'] then lsp_setup.filetypes = config['filetypes'] end
      if config['init_options'] then lsp_setup.init_options = config['init_options'] end
      if config['root_dir'] then lsp_setup.root_dir = config['root_dir'] end
      if config['settings'] then lsp_setup.settings = config['settings'] end
      lspconfig[lsp].setup(lsp_setup)
    end

    vim.api.nvim_create_autocmd('BufWritePre', {
      group = vim.api.nvim_create_augroup('LspFormatOnSave', { clear = true, }),
      callback = function() vim.lsp.buf.format({ async = false, }) end,
    })
  end,
}
