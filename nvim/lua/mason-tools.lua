return {
  lsps = {
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
        completion = {
          callSnippet = 'Both',
          callKeyword = 'Both',
        },
        format = {
          defaultConfig = {
            quote_style = 'single',
            trailing_table_separator = 'always',
            insert_final_newline = 'true',
          },
        },
        hint = { enable = true, setType = true, },
        diagnostics = { globals = { 'vim', }, },
        telemetry = { enable = false, },
        workspace = { checkThirdParty = false, },
      },
    },
    marksman = {},
    ruff_lsp = {},
    rust_analyzer = {
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
    sqlls = {},
    taplo = {},
    tsserver = {},
    yamlls = {},
  },
  others = {
    hadolint = {},
  },
}
