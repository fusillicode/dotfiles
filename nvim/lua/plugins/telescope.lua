return {
  'nvim-telescope/telescope.nvim',
  keys = { '<leader>', 'gd', 'gr', 'gi', },
  branch = 'master',
  dependencies = {
    'nvim-lua/plenary.nvim',
    'nvim-telescope/telescope-live-grep-args.nvim',
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
    local my_global_defaults = {
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
        width = 100,
      },
      preview_title = false,
      prompt_title = false,
      results_title = false,
      show_line = false,
    }
    local function with_my_global_defaults(picker, opts)
      return function()
        telescope_builtin[picker](vim.tbl_extend('force', my_global_defaults, opts or {}))
      end
    end

    vim.keymap.set('n', 'gd', with_my_global_defaults('lsp_definitions', { prompt_prefix = 'LSP Def: ', }))
    vim.keymap.set('n', 'gr', with_my_global_defaults('lsp_references', { prompt_prefix = 'LSP Ref: ', }))
    vim.keymap.set('n', 'gi', with_my_global_defaults('lsp_implementations', { prompt_prefix = 'LSP Impl: ', }))
    vim.keymap.set('n', '<leader>s', with_my_global_defaults('lsp_document_symbols', { prompt_prefix = 'LSP Symbol: ', }))
    vim.keymap.set('n', '<leader>S',
      with_my_global_defaults('lsp_dynamic_workspace_symbols', { prompt_prefix = 'LSP Symbol Workspace: ', }))
    vim.keymap.set('n', '<leader>b', with_my_global_defaults('buffers', { prompt_prefix = 'Buffer: ', }))
    vim.keymap.set('n', '<leader>f', with_my_global_defaults('find_files', { prompt_prefix = 'File: ', }))
    vim.keymap.set('n', '<leader>j', with_my_global_defaults('jumplist', { prompt_prefix = 'Jump: ', }))
    vim.keymap.set('n', '<leader>gc', with_my_global_defaults('git_commits', { prompt_prefix = 'Git Commit: ', }))
    vim.keymap.set('n', '<leader>gcb',
      with_my_global_defaults('git_bcommits', { prompt_prefix = ' Git Commit Buffer >', bufnr = 0, }))
    vim.keymap.set('n', '<leader>gb', with_my_global_defaults('git_branches', { prompt_prefix = 'Git Branch: ', }))
    vim.keymap.set('n', '<leader>gs', with_my_global_defaults('git_status', { prompt_prefix = 'Git Status: ', }))
    vim.keymap.set('n', '<leader>d',
      with_my_global_defaults('diagnostics', { prompt_prefix = 'Diagnostic: ', bufnr = 0, }))
    vim.keymap.set('n', '<leader>D',
      with_my_global_defaults('diagnostics', { prompt_prefix = 'Diagnostic Workspace: ', }))
    vim.keymap.set('n', '<leader>hh', with_my_global_defaults('help_tags', { prompt_prefix = 'Help tag: ', }))
    vim.keymap.set('n', '<leader>l', ':Telescope resume<CR>')
    vim.keymap.set('n', '<leader>/', function()
      telescope.extensions.egrepify.egrepify(vim.tbl_extend('force', my_global_defaults, { prompt_prefix = 'rg: ', }))
    end)
    vim.keymap.set('n', '<leader>T', ':TodoTelescope<CR>')

    telescope.setup({
      defaults = vim.tbl_extend('force', require('telescope.themes').get_dropdown(), my_global_defaults),
      extensions = {
        egrepify = {
          prefixes = {
            ['.'] = { flag = 'hidden', },
          },
        },
      },
      pickers = {
        find_files = {
          find_command = { 'rg', '--files', '--hidden', '--glob', '!**/.git/*', },
        },
      },
    })

    telescope.load_extension('fzf')
    telescope.load_extension('egrepify')

    require('telescope._extensions.egrepify.config').values.mappings = {}
  end,
}
