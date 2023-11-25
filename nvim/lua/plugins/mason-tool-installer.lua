return {
  'WhoIsSethDaniel/mason-tool-installer.nvim',
  dependencies = {
    'williamboman/mason.nvim',
  },
  -- cmd = {
  --   'Mason',
  --   'MasonInstall',
  --   'MasonUpdate',
  --   'MasonUninstall',
  --   'MasonToolsInstall',
  --   'MasonToolsUpdate',
  --   'MasonToolsClean',
  -- },
  config = function()
    require('mason').setup({})

    local mason_tool_installer = require('mason-tool-installer')

    local mason_tools = {}
    for _, tools in pairs(require('../mason-tools')) do
      vim.list_extend(mason_tools, vim.tbl_keys(tools))
    end
    vim.api.nvim_create_autocmd('User', {
      pattern = 'MasonToolsStartingInstall',
      callback = function()
        vim.schedule(function()
          print 'mason-tool-installer is starting'
        end)
      end,
    })
    vim.api.nvim_create_autocmd('User', {
      pattern = 'MasonToolsUpdateCompleted',
      callback = function(e)
        vim.schedule(function()
          print(vim.inspect(e.data)) -- print the table that lists the programs that were installed
        end)
      end,
    })
    mason_tool_installer.setup({ ensure_installed = mason_tools, run_on_start = false, })
    mason_tool_installer.run_on_start()
  end,
}
