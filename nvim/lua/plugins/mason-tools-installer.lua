return {
  'WhoIsSethDaniel/mason-tool-installer.nvim',
  cmd = { 'MasonToolsInstall', 'MasonToolsUpdate', 'MasonToolsClean', },
  config = function()
    require('mason-tool-installer').setup({
      ensure_installed = vim.tbl_keys(require('../lsps')),
    })
  end,
}
