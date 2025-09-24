local function get_custom_lsps_configs()
  local schemastore = require('schemastore')

  return {
    bashls = {},
    docker_compose_language_service = {},
    dockerls = {},
    elixirls = {
      cmd = { vim.fn.expand('~/.local/bin/elixir-ls'), },
      settings = {
        elixirLS = {
          signatureAfterComplete = true,
          suggestSpecs = true,
        },
      },
    },
    elmls = {},
    graphql = {},
    harper_ls = {
      settings = {
        ['harper-ls'] = {
          linters = {
            LongSentences = false,
          },
          userDictPath = vim.fn.expand('~/.config/harper-ls/dictionary.txt'),
          dialect = 'British',
        },
      },
    },
    html = {
      filetypes = { 'html', 'htmldjango', },
      on_attach = function(client, _)
        client.server_capabilities.documentFormattingProvider = false
      end,
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
      cmd = { vim.fn.expand('~/.dev-tools/lua-language-server/bin/lua-language-server'), },
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
    ruff = {},
    rust_analyzer = {
      settings = {
        ['rust-analyzer'] = {
          cargo = {
            build_script = { enable = true, },
            extraArgs = { '--profile', 'rust-analyzer', },
            extraEnv = { CARGO_PROFILE_RUST_ANALYZER_INHERITS = 'dev', },
            allTargets = true,
            allFeatures = true,
          },
          check = {
            command = 'clippy',
          },
          checkOnSave = true,
          completion = { autoimport = { enable = true, }, },
          diagnostics = {
            enable = true,
            disabled = { 'unresolved-proc-macro', },
            styleLints = { enable = true, },
          },
          files = { excludeDirs = { 'target', }, },
          imports = { enforce = true, granularity = { group = 'item', }, prefix = 'crate', },
          lens = { debug = { enable = false, }, implementations = { enable = false, }, run = { enable = false, }, },
          procMacro = { enable = true, },
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
    ts_ls = {
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

local style_opts = require('nvrim').style_opts

return {
  'neovim/nvim-lspconfig',
  event = { 'BufReadPre', 'BufNewFile', },
  dependencies = {
    'saghen/blink.cmp',
    'b0o/schemastore.nvim',
  },
  config = function()
    local blink_cmp = require('blink.cmp')
    local keymaps   = require('keymaps').lspconfig


    for lsp, custom_config in pairs(get_custom_lsps_configs()) do
      custom_config.capabilities = blink_cmp.get_lsp_capabilities(custom_config.capabilities)
      local custom_on_attach = custom_config['on_attach']
      custom_config.on_attach = function(client, bufnr)
        keymaps(bufnr)
        if custom_on_attach then custom_on_attach(client, bufnr) end
      end
      vim.lsp.config[lsp] = custom_config
      vim.lsp.enable(lsp)
    end

    -- To show border with Shift-k (K)
    -- https://www.reddit.com/r/neovim/comments/1jbegzo/how_to_change_border_style_in_floating_windows/
    local orig_lsp_util_open_floating_preview = vim.lsp.util.open_floating_preview
    function vim.lsp.util.open_floating_preview(contents, syntax, opts, ...)
      opts = opts or {}
      opts.border = opts.border or style_opts.window.border
      return orig_lsp_util_open_floating_preview(contents, syntax, opts, ...)
    end

    -- https://vinnymeller.com/posts/neovim_nightly_inlay_hints/#globally
    vim.api.nvim_create_autocmd('LspAttach', {
      group = vim.api.nvim_create_augroup('LspAttachInlayHints', { clear = true, }),
      callback = function(args)
        if not (args.data and args.data.client_id) then
          return
        end

        local client = vim.lsp.get_client_by_id(args.data.client_id)
        if client.server_capabilities.inlayHintProvider then
          vim.lsp.inlay_hint.enable(true, { args.buf, })
        end
      end,
    })

    vim.api.nvim_create_autocmd('BufWritePre', {
      group = vim.api.nvim_create_augroup('LspFormatOnSave', { clear = true, }),
      callback = function() vim.lsp.buf.format({ async = false, }) end,
    })
  end,
}
