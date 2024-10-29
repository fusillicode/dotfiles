local log_file = vim.lsp.get_log_path()

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
          check = { command = 'check', },
          checkOnSave = { command = 'check', },
          completion = { autoimport = { enable = true, }, },
          diagnostics = {
            enable = true,
            disabled = { 'unresolved-proc-macro', },
            styleLints = { enable = true, },
          },
          imports = { enforce = true, granularity = { group = 'item', }, prefix = 'crate', },
          lens = { debug = { enable = false, }, implementations = { enable = false, }, run = { enable = false, }, },
          procMacro = { enable = true, },
          showUnlinkedFileNotification = false,
        },
      },
      -- on_attach = function(client, _)
      --   client.handlers['textDocument/publishDiagnostics'] = function(_, result, ctx, config)
      --     local file = io.open(log_file, 'a')
      --     if file then
      --       local bufnr = vim.uri_to_bufnr(result.uri)
      --       if not vim.api.nvim_buf_is_loaded(bufnr) then return end
      --
      --       vim.diagnostic.reset(vim.lsp.diagnostic.get_namespace(ctx.client_id), bufnr)
      --
      --       for idx, diagnostic in ipairs(result.diagnostics) do
      --         if diagnostic.source == 'rustc' then
      --           file:write(vim.inspect(diagnostic))
      --           result.diagnostics[idx] = result.diagnostics[idx]
      --         else
      --           result.diagnostics[idx] = result.diagnostics[idx]
      --         end
      --       end
      --       vim.lsp.diagnostic.on_publish_diagnostics(_, result, ctx, config)
      --       file:close()
      --     end
      --   end
      -- end,
      -- handlers = {
      --   ['textDocument/hover'] = function(_, result, _, config)
      --     if not (result and result.contents) then return end
      --
      --     -- Get the current buffer number and cursor position
      --     local bufnr = vim.api.nvim_get_current_buf()
      --     local line = vim.api.nvim_win_get_cursor(0)[1] - 1 -- Adjust 1-indexed to 0-indexed
      --
      --     -- Retrieve all diagnostics for the current line
      --     local diagnostics = vim.diagnostic.get(bufnr, { lnum = line, })
      --
      --     -- Concatenate the hover information with diagnostics if available
      --     local message = vim.lsp.util.convert_input_to_markdown_lines(result.contents)
      --     if diagnostics and #diagnostics > 0 then
      --       table.insert(message, '\n\n**Rust Diagnostics:**')
      --       for _, diagnostic in ipairs(diagnostics) do
      --         table.insert(message, diagnostic.message)
      --       end
      --     end
      --
      --     -- Display the concatenated hover and diagnostic message
      --     vim.lsp.util.open_floating_preview(message, 'markdown', config)
      --   end,
      -- },
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
    local lspconfig_keymaps = require('keymaps').lspconfig

    for lsp, config in pairs(get_lsps_configs()) do
      -- 🥲 https://neovim.discourse.group/t/cannot-serialize-function-type-not-supported/4542/3
      local lsp_setup = {
        capabilities = capabilities,
        on_attach = function(client, bufnr)
          lspconfig_keymaps(bufnr)
          if config['on_attach'] then config['on_attach'](client, bufnr) end
        end,
      }
      if config['cmd'] then lsp_setup.cmd = config['cmd'] end
      if config['filetypes'] then lsp_setup.filetypes = config['filetypes'] end
      if config['init_options'] then lsp_setup.init_options = config['init_options'] end
      if config['root_dir'] then lsp_setup.root_dir = config['root_dir'] end
      if config['settings'] then lsp_setup.settings = config['settings'] end
      if config['handlers'] then lsp_setup.handlers = config['handlers'] end
      lspconfig[lsp].setup(lsp_setup)
    end

    -- To show border with Shift-k (K)
    vim.lsp.handlers['textDocument/hover'] = vim.lsp.with(
      vim.lsp.handlers.hover,
      { border = 'rounded', }
    )
    vim.lsp.handlers['textDocument/signatureHelp'] = vim.lsp.with(
      vim.lsp.handlers.signature_help,
      { border = 'rounded', }
    )

    -- https://github.com/neovim/neovim/issues/12970
    vim.lsp.util.apply_text_document_edit = function(text_document_edit, _, offset_encoding)
      local text_document = text_document_edit.textDocument
      local bufnr = vim.uri_to_bufnr(text_document.uri)

      if offset_encoding == nil then
        vim.notify_once('apply_text_document_edit must be called with valid offset encoding', vim.log.levels.WARN)
      end

      vim.lsp.util.apply_text_edits(text_document_edit.edits, bufnr, offset_encoding)
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
