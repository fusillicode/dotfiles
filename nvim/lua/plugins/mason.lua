return {
  'williamboman/mason.nvim',
  cmd = 'Mason',
  dependencies = { 'williamboman/mason-lspconfig.nvim', },
  config = function()
    require('mason').setup({})

    local lspconfig_mappings_server = require('mason-lspconfig.mappings.server')
    local mason_registry = require('mason-registry')
    local mason_tools = require('../mason-tools')

    local function install_mason_pkg(pkg_name)
      local ok, pkg = pcall(mason_registry.get_package, pkg_name)

      if not ok then
        print('Error getting pkg ' .. pkg_name .. ' from mason-registry')
        return
      end

      if not pkg:is_installed() then
        print(pkg_name .. ' installing')
        pkg:install()
        print(pkg_name .. ' installed')
      end
    end

    for lspconfig_name, _ in pairs(mason_tools['lsps']) do
      install_mason_pkg(lspconfig_mappings_server.lspconfig_to_package[lspconfig_name])
    end

    for mason_tool_name, _ in pairs(mason_tools['others']) do
      install_mason_pkg(mason_tool_name)
    end
  end,
}
