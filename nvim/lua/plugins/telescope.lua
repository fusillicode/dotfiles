return {
  'nvim-telescope/telescope.nvim',
  keys = { '<leader>', 'gd', 'gr', 'gi', },
  branch = 'master',
  dependencies = {
    'nvim-lua/plenary.nvim',
    'nvim-telescope/telescope-live-grep-args.nvim',
    'nvim-telescope/telescope-file-browser.nvim',
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
    local vkeyset = vim.keymap.set
    local my_global_defaults = {
      mappings = {
        i = {
          ['<C-a>'] = function() vim.cmd 'normal! ^' end,
          ['<C-e>'] = function() vim.cmd 'normal! $' end,
          ['<C-b>'] = function() vim.cmd 'normal! h' end,
          ['<C-f>'] = function() vim.cmd 'normal! l' end,
          ['<A-f>'] = function() vim.cmd 'normal! w' end,
          ['<A-b>'] = function() vim.cmd 'normal! b' end,
          ['<C-k>'] = function() vim.cmd 'normal! d$' end,
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
    local function with_my_default(picker, opts)
      return function()
        telescope_builtin[picker](vim.tbl_extend('force', my_global_defaults, opts or {}))
      end
    end

    vkeyset('n', 'gd', with_my_default('lsp_definitions', { prompt_prefix = 'LSP Def: ', }))
    vkeyset('n', 'gr', with_my_default('lsp_references', { prompt_prefix = 'LSP Ref: ', }))
    vkeyset('n', 'gi', with_my_default('lsp_implementations', { prompt_prefix = 'LSP Impl: ', }))
    vkeyset('n', '<leader>s', with_my_default('lsp_document_symbols', { prompt_prefix = 'LSP Symbol: ', }))
    vkeyset('n', '<leader>S',
      with_my_default('lsp_dynamic_workspace_symbols', { prompt_prefix = 'LSP Symbol Workspace: ', }))
    vkeyset('n', '<leader>b', with_my_default('buffers', { prompt_prefix = 'Buffer: ', }))
    vkeyset('n', '<leader>f', with_my_default('find_files', { prompt_prefix = 'File: ', }))
    vkeyset('n', '<leader>j', with_my_default('jumplist', { prompt_prefix = 'Jump: ', }))
    vkeyset('n', '<leader>gc', with_my_default('git_commits', { prompt_prefix = 'Git Commit: ', }))
    vkeyset('n', '<leader>gbb', with_my_default('git_bcommits', { prompt_prefix = ' Git Commit Buffer >', bufnr = 0, }))
    vkeyset('n', '<leader>gb', with_my_default('git_branches', { prompt_prefix = 'Git Branch: ', }))
    vkeyset('n', '<leader>gs', with_my_default('git_status', { prompt_prefix = 'Git Status: ', }))
    vkeyset('n', '<leader>d', with_my_default('diagnostics', { prompt_prefix = 'Diagnostic: ', bufnr = 0, }))
    vkeyset('n', '<leader>D', with_my_default('diagnostics', { prompt_prefix = 'Diagnostic Workspace: ', }))
    vkeyset('n', '<leader>hh', with_my_default('help_tags', { prompt_prefix = 'Help tag: ', }))
    vkeyset('n', '<leader>l', ':Telescope resume<CR>')
    vkeyset('n', '<leader>/', function()
      telescope.extensions.egrepify.egrepify(vim.tbl_extend('force', my_global_defaults, { prompt_prefix = 'rg: ', }))
    end)
    vkeyset('n', '<leader>F', function()
      telescope.extensions.file_browser.file_browser(
        vim.tbl_extend('force', my_global_defaults,
          { prompt_prefix = 'Dir/File: ', path = '%:p:h', select_buffer = true, })
      )
    end)
    vkeyset('n', '<leader>T', ':TodoTelescope<CR>')

    local file_browser_actions = require('telescope._extensions.file_browser.actions')
    telescope.setup({
      defaults = vim.tbl_extend('force', require('telescope.themes').get_dropdown(), my_global_defaults),
      extensions = {
        egrepify = {
          prefixes = {
            ['h'] = { flag = 'hidden', },
          },
        },
        file_browser = {
          dir_icon = '',
          grouped = true,
          hidden = { file_browser = true, folder_browser = true, },
          hide_parent_dir = true,
          hijack_netrw = true,
          mappings = {
            ['i'] = {
              ['<C-h>'] = file_browser_actions.goto_parent_dir,
            },
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
    telescope.load_extension('file_browser')
    telescope.load_extension('egrepify')
  end,
}
