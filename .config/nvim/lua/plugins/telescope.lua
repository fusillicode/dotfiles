return {
  'nvim-telescope/telescope.nvim',
  keys = { '<leader>', 'gd', 'gr', 'gi', },
  branch = 'master',
  dependencies = {
    'nvim-lua/plenary.nvim',
    'nvim-telescope/telescope-live-grep-args.nvim',
    'nvim-telescope/telescope-ui-select.nvim',
    'fdschmidt93/telescope-egrepify.nvim',
    {
      'nvim-telescope/telescope-fzf-native.nvim',
      build = 'make',
      cond = function() return vim.fn.executable 'make' == 1 end,
    },
  },
  config = function()
    local telescope = require('telescope')
    local telescope_builtin = require('telescope.builtin')
    local actions = require('telescope.actions')
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
          ['<esc>'] = actions.close,
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
    require('keymaps').telescope(telescope, telescope_builtin, defaults)

    local theme = vim.tbl_extend('force', require('telescope.themes').get_dropdown(), defaults)

    telescope.setup({
      defaults = theme,
      extensions = {
        egrepify = {
          prefixes = {
            ['.'] = { flag = 'hidden', },
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

    telescope.load_extension('fzf')
    telescope.load_extension('egrepify')
    telescope.load_extension('ui-select')

    require('telescope._extensions.egrepify.config').values.mappings = {}
    vim.cmd('autocmd User TelescopePreviewerLoaded setlocal number')
  end,
}
