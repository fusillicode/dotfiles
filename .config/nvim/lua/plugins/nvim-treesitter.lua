local keymaps = require('keymaps')
local plugin_keymaps = keymaps.nvim_tree_sitter

return {
  'nvim-treesitter/nvim-treesitter',
  branch = 'main',
  lazy = false,
  dependencies = {
    { 'nvim-treesitter/nvim-treesitter-textobjects', branch = 'main', },
  },
  build = ':TSUpdate',
  config = function()
    local jsonnet_filetypes = { 'jsonnet', 'libsonnet', }

    vim.filetype.add({
      extension = {
        jsonnet = 'jsonnet',
        libsonnet = 'libsonnet',
      },
    })

    require('nvim-treesitter').setup()
    vim.treesitter.language.register('jsonnet', jsonnet_filetypes)

    vim.api.nvim_create_autocmd('FileType', {
      pattern = jsonnet_filetypes,
      callback = function() vim.treesitter.start() end,
    })

    -- ensure_installed equivalent
    require('nvim-treesitter.install').install({
      'bash',
      'comment',
      'css',
      'diff',
      'dockerfile',
      'git_config',
      'git_rebase',
      'gitattributes',
      'gitcommit',
      'gitignore',
      'graphql',
      'html',
      'javascript',
      'json',
      'jsonnet',
      'kdl',
      'lua',
      'make',
      'markdown',
      'markdown_inline',
      'mermaid',
      'python',
      'regex',
      'rust',
      'sql',
      'textproto',
      'toml',
      'typescript',
      'vim',
      'vimdoc',
      'xml',
      'yaml',
    })

    keymaps.set(plugin_keymaps())
  end,
}
