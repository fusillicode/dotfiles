return {
  'williamboman/mason.nvim',
  cmd = 'Mason',
  dependencies = { 'williamboman/mason-lspconfig.nvim', },
  config = function()
    require('mason').setup({})

    local lspconfig_mappings_server = require('mason-lspconfig.mappings.server')
    local mason_registry = require('mason-registry')
    local mason_tools = require('../mason-tools')

    local function install_mason_package(package_registry_name)
      local ok, pkg = pcall(mason_registry.get_package, package_registry_name)
      if ok then
        if not pkg:is_installed() then
          pkg:install()
        end
      end
    end

    for lspconfig_name, _ in pairs(mason_tools['lsps']) do
      install_mason_package(lspconfig_mappings_server.lspconfig_to_package[lspconfig_name])
    end

    for mason_tool_name, _ in pairs(mason_tools['others']) do
      install_mason_package(mason_tool_name)
    end
  end,
}
