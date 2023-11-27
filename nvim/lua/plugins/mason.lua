return {
  'williamboman/mason.nvim',
  cmd = 'Mason',
  dependencies = { 'williamboman/mason-lspconfig.nvim', },
  config = function()
    require('mason').setup({})

    local lspconfig_mappings_server = require('mason-lspconfig.mappings.server')
    local mason_registry = require('mason-registry')
    local mason_tools = require('../mason-tools')

    local function sync_mason_pkg(installed_tools)
      return function(pkg_name)
        local ok, pkg = pcall(mason_registry.get_package, pkg_name)
        if not ok then return end
        print(vim.inspect(pkg.name) .. ' ' .. vim.inspect(pkg_name))
        print(vim.inspect(installed_tools))
        print(vim.inspect(pkg:is_installed()))

        if pkg:is_installed() and not installed_tools[pkg_name] then
          return pkg:uninstall()
        end

        if not pkg:is_installed() then
          return pkg:install()
        end
      end
    end

    local installed_pkgs = mason_registry.get_installed_package_names()

    for lspconfig_name, _ in pairs(mason_tools['lsps']) do
      sync_mason_pkg(installed_pkgs)(lspconfig_mappings_server.lspconfig_to_package[lspconfig_name])
    end

    for mason_tool_name, _ in pairs(mason_tools['others']) do
      sync_mason_pkg(installed_pkgs)(mason_tool_name)
    end
  end,
}
