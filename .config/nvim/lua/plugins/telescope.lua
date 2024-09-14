return {
  'nvim-telescope/telescope.nvim',
  keys = { '<leader>', 'gd', 'gr', 'gi', },
  branch = 'master',
  dependencies = {
    'nvim-lua/plenary.nvim',
    'nvim-telescope/telescope-ui-select.nvim',
    {
      'nvim-telescope/telescope-fzf-native.nvim',
      build = 'make',
      cond = function() return vim.fn.executable 'make' == 1 end,
    },
    'nvim-telescope/telescope-live-grep-args.nvim',
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
        anchor = 'N',
        height = 0.40,
        prompt_position = 'top',
        width = 0.8,
      },
      layout_strategy = 'center',
      path_display = { 'filename_first', },
      preview_title = false,
      prompt_title = false,
      results_title = false,
      show_line = false,
    }
    local telescope = require('telescope')
    local lga_actions = require('telescope-live-grep-args.actions')
    require('keymaps').telescope(require('telescope.builtin'), defaults)

    local defaults_and_theme = vim.tbl_extend('force', require('telescope.themes').get_dropdown(), defaults)
    telescope.setup({
      defaults = defaults_and_theme,
      extensions = {
        ['ui-select'] = { defaults_and_theme, },
        ['live_grep_args'] = {
          auto_quoting = true,
          mappings = {
            i = {
              ['<C-w>'] = lga_actions.quote_prompt(),
              ['<C-i>'] = lga_actions.quote_prompt({ postfix = ' --iglob ', }),
              ['<C-space>'] = lga_actions.to_fuzzy_refine,
            },
          },
        },
      },
      pickers = {
        buffers = {
          ignore_current_buffer = true,
          sort_lastused = true,
          sort_mru = true,
        },
        find_files = {
          find_command = {
            'fd',
            '--color=never',
            '--type=f',
            '--hidden',
            '--follow',
            '--no-ignore-vcs',
            '--exclude=**/.git/*',
            '--exclude=**/target/*',
            '--exclude=**/_build/*',
            '--exclude=**/deps/*',
            '--exclude=**/.elixir_ls/*',
            '--exclude=**/node_modules/*',
          },
        },
      },
    })

    telescope.load_extension('ui-select')
    telescope.load_extension('fzf')
    telescope.load_extension('attempt')
    telescope.load_extension('live_grep_args')

    vim.cmd('autocmd User TelescopePreviewerLoaded setlocal number')
  end,
}
