return {
  'nvim-telescope/telescope.nvim',
  keys = { '<leader>', 'gd', 'gr', 'gi', },
  branch = 'master',
  dependencies = {
    'nvim-lua/plenary.nvim',
    'nvim-telescope/telescope-ui-select.nvim',
    'nvim-telescope/telescope-live-grep-args.nvim',
    {
      'nvim-telescope/telescope-fzf-native.nvim',
      build = 'make',
      cond = function() return vim.fn.executable 'make' == 1 end,
    },
  },
  config = function()
    local defaults = {
      mappings = {
        ['i'] = {
          ['<C-a>'] = function() vim.cmd('normal! ^') end,
          ['<C-e>'] = function() vim.cmd('normal! $') end,
          ['<C-b>'] = function() vim.cmd('normal! h') end,
          ['<C-f>'] = function() vim.cmd('normal! l') end,
          ['<A-f>'] = function() vim.cmd('normal! w') end,
          ['<A-b>'] = function() vim.cmd('normal! b') end,
          ['<C-k>'] = function() vim.cmd('normal! d$') end,
          ['<esc>'] = require('telescope.actions').close,
        },
      },
      layout_config = {
        anchor = 'S',
        width = 90,
      },
      preview_title = false,
      prompt_title = false,
      results_title = false,
      show_line = false,
    }
    local telescope = require('telescope')
    require('keymaps').telescope(telescope, require('telescope.builtin'), defaults)

    local theme = vim.tbl_extend('force', require('telescope.themes').get_dropdown(), defaults)
    telescope.setup({
      defaults = theme,
      extensions = {
        egrepify = {
          prefixes = {
            ['.'] = { flag = 'hidden', },
          },
        },
        live_grep_args = {
          prompt_title = false,
          vimgrep_arguments = {
            'rg',
            '--color=never',
            '--column',
            '--line-number',
            '--no-heading',
            '--smart-case',
            '--with-filename',
            '--hidden',
            '--glob',
            '!**/.git/*',
          },
        },
        ['ui-select'] = { theme, },
      },
      pickers = {
        find_files = {
          find_command = { 'rg', '--files', '--hidden', '--glob', '!**/.git/*', },
        },
      },
    })

    telescope.load_extension('ui-select')
    telescope.load_extension('live_grep_args')
    telescope.load_extension('fzf')

    vim.cmd('autocmd User TelescopePreviewerLoaded setlocal number')
  end,
}
