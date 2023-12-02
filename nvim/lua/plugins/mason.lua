---@diagnostic disable-next-line: unused-local, unused-function
local function dbg(foo)
  print(vim.inspect(foo))
  return foo
end

local function set_diff(s1, s2)
  local diff = {}
  for k, v in pairs(s1) do if s2[k] == nil then diff[k] = v end end
  return diff
end

local function new_set(table)
  local set = {}
  for _, v in ipairs(table) do set[v] = true end
  return set
end

local function get_mason_pkg(mason_registry, pkg_name)
  local ok, pkg = pcall(mason_registry.get_package, pkg_name)

  if not ok then
    print('Error getting pkg ' .. pkg_name .. ' from mason-registry')
    return
  end

  return pkg
end

local function install_mason_pkg(pkg)
  if pkg:is_installed() then
    print(pkg.name .. ' already installed')
    return
  end
  print(pkg.name .. ' installing')
  pkg:install()
  print(pkg.name .. ' installed')
end

local function uninstall_mason_pkg(pkg)
  if not pkg:is_installed() then
    print(pkg.name .. ' already not installed')
    return
  end
  print(pkg.name .. ' uninstalling')
  pkg:uninstall()
  print(pkg.name .. ' uninstalled')
end

return {
  'williamboman/mason.nvim',
  cmd = 'Mason',
  dependencies = { 'williamboman/mason-lspconfig.nvim', },
  config = function()
    require('mason').setup({})

    local lspconfig_mappings_server = require('mason-lspconfig.mappings.server')
    local mason_registry = require('mason-registry')
    local mason_tools = require('../mason-tools')

    local pkgs_names = {}
    for mason_tool_name, _ in pairs(vim.tbl_extend('error', mason_tools['lsps'], mason_tools['others'])) do
      local pkg_name = lspconfig_mappings_server.lspconfig_to_package[mason_tool_name] or mason_tool_name
      pkgs_names[pkg_name] = true
    end

    local installed_pkgs_names = new_set(mason_registry.get_installed_package_names())

    for pkg_name_to_install in pairs(set_diff(pkgs_names, installed_pkgs_names)) do
      local pkg = get_mason_pkg(mason_registry, pkg_name_to_install)
      if pkg then install_mason_pkg(pkg) end
    end

    for pkg_name_to_uninstall in pairs(set_diff(installed_pkgs_names, pkgs_names)) do
      local pkg = get_mason_pkg(mason_registry, pkg_name_to_uninstall)
      if pkg then uninstall_mason_pkg(pkg) end
    end
  end,
}
