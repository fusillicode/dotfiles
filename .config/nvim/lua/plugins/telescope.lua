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
    local telescope_actions = require('telescope.actions')
    local defaults = {
      mappings = {
        ['i'] = {
          ['<c-a>'] = function() vim.cmd('normal! ^') end,
          ['<c-e>'] = function() vim.cmd('normal! $') end,
          ['<c-w>'] = function() vim.cmd('normal! w') end,
          ['<c-b>'] = function() vim.cmd('normal! b') end,
          ['<c-k>'] = function() vim.cmd('normal! d$') end,
          ['<c-f>'] = telescope_actions.to_fuzzy_refine,
          ['<esc>'] = telescope_actions.close,
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
      dynamic_preview_title = true,
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
              ['<c-s>'] = lga_actions.quote_prompt(),
              ['<c-i>'] = lga_actions.quote_prompt({ postfix = ' --iglob ', }),
              ['<c-f>'] = telescope_actions.to_fuzzy_refine,
            },
          },
          vimgrep_arguments = {
            'rg',
            '--color=never',
            '--no-heading',
            '--with-filename',
            '--line-number',
            '--column',
            '--smart-case',
            '--hidden',
            '--glob=!**/.git/*',
            '--glob=!**/target/*',
            '--glob=!**/_build/*',
            '--glob=!**/deps/*',
            '--glob=!**/.elixir_ls/*',
            '--glob=!**/node_modules/*',
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
            '--follow',
            '--no-ignore-vcs',
            '--hidden',
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
