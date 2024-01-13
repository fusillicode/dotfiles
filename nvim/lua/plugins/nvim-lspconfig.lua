local function get_lsps_configs(lsps_common_configs)
  local home_dir = os.getenv('HOME')
  local lsps_configs = {
    bashls = {},
    docker_compose_language_service = {},
    dockerls = {},
    elixirls = {
      cmd = { home_dir .. '/.local/bin/elixir-ls', },
    },
    elmls = {},
    graphql = {},
    html = {},
    helm_ls = {},
    jsonls = {
      settings = {
        json = {
          validate = { enable = true, },
          schemas = require('schemastore').json.schemas({
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
      settings = {
        psalm = {
          configPaths = { 'psalm.xml', 'psalm.xml.dist', 'psalm-baseline.xml', },
        },
      },
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
          imports = { enforce = true, granularity = { group = 'item', }, prefix = 'crate', },
          lens = { debug = { enable = false, }, implementations = { enable = false, }, run = { enable = false, }, },
          proc_macro = { enable = true, },
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
    tsserver = {},
    yamlls = {
      settings = {
        yaml = {
          schemaStore = {
            enable = false,
            url = '',
          },
          schemas = vim.tbl_extend('error',
            require('schemastore').yaml.schemas({
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

  for lsp, lsp_config in pairs(lsps_configs) do
    lsp_config[lsp] = vim.tbl_extend('error', lsps_common_configs, lsp_config)
  end

  return lsps_configs
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
    local keymap_set = require('utils').keymap_set

    local function on_attach(_, bufnr)
      keymap_set('', '<c-r>', ':LspRestart<cr>')
      keymap_set('n', 'K', vim.lsp.buf.hover, { buffer = bufnr, })
      keymap_set('n', '<leader>r', vim.lsp.buf.rename, { buffer = bufnr, })
    end

    for lsp, config in pairs(get_lsps_configs({ capabilities = capabilities, on_attach = on_attach, })) do
      lspconfig[lsp].setup(config)
    end

    vim.api.nvim_create_autocmd('BufWritePre', {
      group = vim.api.nvim_create_augroup('LspFormatOnSave', { clear = true, }),
      callback = function() vim.lsp.buf.format({ async = false, }) end,
    })
  end,
}
