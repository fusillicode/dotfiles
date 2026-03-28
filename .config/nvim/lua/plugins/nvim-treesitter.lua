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
    require('nvim-treesitter').setup()

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
