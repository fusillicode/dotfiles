return {
  'nvim-treesitter/nvim-treesitter',
  event = { 'BufReadPost', },
  dependencies = { 'nvim-treesitter/nvim-treesitter-textobjects', },
  build = ':TSUpdate',
  config = function()
    require('nvim-treesitter.configs').setup({
      auto_install = false,
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
      highlight = { enable = true, additional_vim_regex_highlighting = false, },
      incremental_selection = {
        enable = true,
        keymaps = {
          init_selection = '<CR>',
          scope_incremental = '<CR>',
          node_incremental = '<TAB>',
          node_decremental = '<S-TAB>',
        },
      },
      matchup = { enable = true, enable_quotes = true, },
      sync_install = true,
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
