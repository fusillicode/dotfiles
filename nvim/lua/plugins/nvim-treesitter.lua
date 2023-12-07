return {
  'nvim-treesitter/nvim-treesitter',
  event = { 'BufReadPost', },
  dependencies = { 'nvim-treesitter/nvim-treesitter-textobjects', },
  build = ':TSUpdate',
  config = function()
    require('nvim-treesitter.configs').setup({
      matchup = { enable = true, enable_quotes = true, },
      ensure_installed = {
        'bash',
        'comment',
        'css',
        'diff',
        'dockerfile',
        'elixir',
        'elm',
        'html',
        'javascript',
        'json',
        'kdl',
        'lua',
        'make',
        'markdown',
        'python',
        'regex',
        'rust',
        'sql',
        'textproto',
        'toml',
        'typescript',
        'xml',
        'yaml',
      },
      sync_install = true,
      auto_install = false,
      highlight = { enable = true, additional_vim_regex_highlighting = false, },
      textobjects = {
        move = {
          enable = true,
          set_jumps = true,
          goto_next_start = {
            ['<C-l>'] = '@block.outer',
          },
          goto_previous_end = {
            ['<C-h>'] = '@block.outer',
          },
        },
      },
    })
  end,
}
