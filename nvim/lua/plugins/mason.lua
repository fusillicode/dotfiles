return {
  'williamboman/mason.nvim',
  cmd = 'Mason',
  dependencies = { 'williamboman/mason-lspconfig.nvim', },
  config = function()
    require('mason').setup({})
    local server_mapping = require('mason-lspconfig.mappings.server')
    local registry = require('mason-registry')

    for _, pkg_name in pairs(vim.tbl_keys(require('../lsps'))) do
      local registry_name = server_mapping.lspconfig_to_package[pkg_name] or pkg_name
      local ok, pkg = pcall(registry.get_package, registry_name)
      if ok then
        if not pkg:is_installed() then
          pkg:install()
        end
      end
    end
  end,
}
