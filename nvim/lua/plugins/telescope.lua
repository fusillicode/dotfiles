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
    local keymap_set = require('utils').keymap_set
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

    keymap_set('n', 'gd', with_my_global_defaults('lsp_definitions', { prompt_prefix = 'LSP Def: ', }))
    keymap_set('n', 'gr', with_my_global_defaults('lsp_references', { prompt_prefix = 'LSP Ref: ', }))
    keymap_set('n', 'gi', with_my_global_defaults('lsp_implementations', { prompt_prefix = 'LSP Impl: ', }))
    keymap_set('n', '<leader>s', with_my_global_defaults('lsp_document_symbols', { prompt_prefix = 'LSP Symbol: ', }))
    keymap_set('n', '<leader>S',
      with_my_global_defaults('lsp_dynamic_workspace_symbols', { prompt_prefix = 'LSP Symbol Workspace: ', }))
    keymap_set('n', '<leader>b', with_my_global_defaults('buffers', { prompt_prefix = 'Buffer: ', }))
    keymap_set('n', '<leader>f', with_my_global_defaults('find_files', { prompt_prefix = 'File: ', }))
    keymap_set('n', '<leader>j', with_my_global_defaults('jumplist', { prompt_prefix = 'Jump: ', }))
    keymap_set('n', '<leader>gc', with_my_global_defaults('git_commits', { prompt_prefix = 'Git Commit: ', }))
    keymap_set('n', '<leader>gcb',
      with_my_global_defaults('git_bcommits', { prompt_prefix = ' Git Commit Buffer >', bufnr = 0, }))
    keymap_set('n', '<leader>gb', with_my_global_defaults('git_branches', { prompt_prefix = 'Git Branch: ', }))
    keymap_set('n', '<leader>gs', with_my_global_defaults('git_status', { prompt_prefix = 'Git Status: ', }))
    keymap_set('n', '<leader>d',
      with_my_global_defaults('diagnostics', { prompt_prefix = 'Diagnostic: ', bufnr = 0, }))
    keymap_set('n', '<leader>D',
      with_my_global_defaults('diagnostics', { prompt_prefix = 'Diagnostic Workspace: ', }))
    keymap_set('n', '<leader>hh', with_my_global_defaults('help_tags', { prompt_prefix = 'Help tag: ', }))
    keymap_set('n', '<leader>/', function()
      telescope.extensions.egrepify.egrepify(vim.tbl_extend('force', my_global_defaults, { prompt_prefix = 'rg: ', }))
    end)
    keymap_set('n', '<leader>T', ':TodoTelescope<CR>')
    keymap_set('n', '<leader>l', telescope_builtin.resume)

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
        diagnostics = {
          line_width = 'full',
        },
      },
    })

    telescope.load_extension('fzf')
    telescope.load_extension('egrepify')

    require('telescope._extensions.egrepify.config').values.mappings = {}
    vim.cmd('autocmd User TelescopePreviewerLoaded setlocal number')
  end,
}
