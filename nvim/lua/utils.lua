local M = {}

local mason_tools_pkg_name = '../mason-tools'

function M.sync_mason_tools()
  local lspconfig_mappings_server = require('mason-lspconfig.mappings.server')
  local mason_registry = require('mason-registry')
  package.loaded[mason_tools_pkg_name] = nil
  local mason_tools = require(mason_tools_pkg_name)

  local pkgs_names = {}
  for mason_tool_name, _ in pairs(vim.tbl_extend('error', mason_tools['lsps'], mason_tools['others'])) do
    local pkg_name = lspconfig_mappings_server.lspconfig_to_package[mason_tool_name] or mason_tool_name
    pkgs_names[pkg_name] = true
  end

  local installed_pkgs_names = M.new_set(mason_registry.get_installed_package_names())

  local pkgs_to_install = M.set_diff(pkgs_names, installed_pkgs_names)
  for pkg_to_install in pairs(pkgs_to_install) do
    local pkg = M.get_mason_pkg(mason_registry, pkg_to_install)
    if pkg then M.install_mason_pkg(pkg) end
  end

  local pkgs_to_uninstall = M.set_diff(installed_pkgs_names, pkgs_names)
  for pkg_to_uninstall in pairs(pkgs_to_uninstall) do
    local pkg = M.get_mason_pkg(mason_registry, pkg_to_uninstall)
    if pkg then M.uninstall_mason_pkg(pkg) end
  end

  if #pkgs_to_install == 0 and #pkgs_to_uninstall == 0 then
    print('Everything already synced')
  end
end

function M.dbg(foo)
  print(vim.inspect(foo))
  return foo
end

function M.set_diff(s1, s2)
  local diff = {}
  for k, v in pairs(s1) do if s2[k] == nil then diff[k] = v end end
  return diff
end

function M.new_set(table)
  local set = {}
  for _, v in ipairs(table) do set[v] = true end
  return set
end

function M.get_mason_pkg(mason_registry, pkg_name)
  local ok, pkg = pcall(mason_registry.get_package, pkg_name)

  if not ok then
    print('Error getting pkg ' .. pkg_name .. ' from mason-registry')
    return
  end

  return pkg
end

function M.install_mason_pkg(pkg)
  if pkg:is_installed() then
    print('[ ' .. pkg.name .. ' ]' .. ' already installed')
    return
  end
  print('[ ' .. pkg.name .. ' ]' .. ' installing')
  pkg:install()
  print('[ ' .. pkg.name .. ' ]' .. ' installed')
end

function M.uninstall_mason_pkg(pkg)
  if not pkg:is_installed() then
    print('[ ' .. pkg.name .. ' ]' .. ' already not installed')
    return
  end
  print('[ ' .. pkg.name .. ' ]' .. ' uninstalling')
  pkg:uninstall()
  print('[ ' .. pkg.name .. ' ]' .. ' uninstalled')
end

return M
