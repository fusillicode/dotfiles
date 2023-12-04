local M = {}

M.tools = {
  lsps = {
    bashls = {},
    docker_compose_language_service = {},
    dockerls = {},
    graphql = {},
    html = {},
    helm_ls = {},
    jsonls = {},
    lua_ls = {
      settings = {
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
    },
    marksman = {},
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
    yamlls = {},
  },
  others = {
    hadolint = {},
  },
}


local function get_mason_pkg(mason_registry, pkg_name)
  local ok, pkg = pcall(mason_registry.get_package, pkg_name)
  if not ok then
    print('❌ Error getting pkg "' .. pkg_name .. '" from mason-registry')
    return
  end
  return pkg
end

local function install_mason_pkg(pkg)
  if pkg:is_installed() then
    print('⚠️ "' .. pkg.name .. '" already installed')
    return
  end
  pkg:install()
  print('✅ ' .. pkg.name .. ' installed')
end

local function uninstall_mason_pkg(pkg)
  if not pkg:is_installed() then
    print('⚠️ "' .. pkg.name .. '" already not installed')
    return
  end
  pkg:uninstall()
  print('✅ "' .. pkg.name .. '" uninstalled')
end

function M.sync()
  local lspconfig_mappings_server = require('mason-lspconfig.mappings.server')
  local utils = require('utils')
  package.loaded['mason-registry'] = nil
  local mason_registry = require('mason-registry')
  package.loaded['mason-tools'] = nil
  local mason_tools = require('mason-tools')

  local pkgs_names = {}
  for mason_tool_name, _ in pairs(vim.tbl_extend('error', mason_tools['tools']['lsps'], mason_tools['tools']['others'])) do
    local pkg_name = lspconfig_mappings_server.lspconfig_to_package[mason_tool_name] or mason_tool_name
    pkgs_names[pkg_name] = true
  end

  local installed_pkgs_names = utils.new_set(mason_registry.get_installed_package_names())

  local pkgs_to_install = utils.set_diff(pkgs_names, installed_pkgs_names)
  for pkg_to_install in pairs(pkgs_to_install) do
    local pkg = get_mason_pkg(mason_registry, pkg_to_install)
    if pkg then install_mason_pkg(pkg) end
  end

  local pkgs_to_uninstall = utils.set_diff(installed_pkgs_names, pkgs_names)
  for pkg_to_uninstall in pairs(pkgs_to_uninstall) do
    local pkg = get_mason_pkg(mason_registry, pkg_to_uninstall)
    if pkg then uninstall_mason_pkg(pkg) end
  end

  if not next(pkgs_to_install) and not next(pkgs_to_uninstall) then
    print("👍 Everything's already synced")
  end
end

return M
