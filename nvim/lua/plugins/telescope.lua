return {
  'nvim-telescope/telescope.nvim',
  keys = { '<leader>', 'gd', 'gr', 'gi' },
  branch = '0.1.x',
  dependencies = {
    'nvim-lua/plenary.nvim',
    'nvim-telescope/telescope-live-grep-args.nvim',
    'nvim-telescope/telescope-file-browser.nvim',
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
    local custom_theme_opts = {
      show_line = false,
      prompt_title = false,
      results_title = false,
      preview_title = false,
    }
    local function custom_theme(picker, opts)
      return function()
        telescope_builtin[picker](vim.tbl_extend('force', custom_theme_opts, opts or {}))
      end
    end

    vkeyset('n', 'gd', custom_theme('lsp_definitions', { prompt_prefix = 'LSP Defs > ' }))
    vkeyset('n', 'gr', custom_theme('lsp_references', { prompt_prefix = 'LSP Refs > ' }))
    vkeyset('n', 'gi', custom_theme('lsp_implementations', { prompt_prefix = 'LSP Impls > ' }))
    vkeyset('n', '<leader>s', custom_theme('lsp_document_symbols', { prompt_prefix = 'LSP Symbols Buffer > ' }))
    vkeyset('n', '<leader>S',
      custom_theme('lsp_dynamic_workspace_symbols', { prompt_prefix = 'LSP Symbols Workspace > ' }))
    vkeyset('n', '<leader>b', custom_theme('buffers', { prompt_prefix = 'Buffers > ' }))
    vkeyset('n', '<leader>f', custom_theme('find_files', { prompt_prefix = 'Files > ' }))
    vkeyset('n', '<leader>j', custom_theme('jumplist', { prompt_prefix = 'Jumps > ' }))
    vkeyset('n', '<leader>c', custom_theme('git_commits', { prompt_prefix = 'Git Commits > ' }))
    vkeyset('n', '<leader>bc', custom_theme('git_bcommits', { prompt_prefix = ' Git Commits Buffer >', bufnr = 0 }))
    vkeyset('n', '<leader>gb', custom_theme('git_branches', { prompt_prefix = 'Git Branches > ' }))
    vkeyset('n', '<leader>gs', custom_theme('git_status', { prompt_prefix = 'Git Status > ' }))
    vkeyset('n', '<leader>d', custom_theme('diagnostics', { prompt_prefix = 'Diagnostics Buffer >', bufnr = 0 }))
    vkeyset('n', '<leader>D', custom_theme('diagnostics', { prompt_prefix = 'Diagnostics Workspace > ' }))
    vkeyset('n', '<leader>l',
      function()
        telescope.extensions.live_grep_args.live_grep_args(
          vim.tbl_extend('force', custom_theme_opts, { prompt_prefix = 'rg > ' })
        )
      end)
    vkeyset('n', '<leader>F', ':Telescope file_browser path=%:p:h select_buffer=true<CR>')
    vkeyset('n', '<leader>t', ':TodoTelescope<CR>')

    local file_browser_actions = require('telescope._extensions.file_browser.actions')
    telescope.setup({
      defaults = vim.tbl_extend('force', require('telescope.themes').get_dropdown(), custom_theme_opts),
      extensions = {
        file_browser = {
          dir_icon = '',
          grouped = true,
          hidden = { file_browser = true, folder_browser = true },
          hide_parent_dir = true,
          hijack_netrw = true,
          mappings = {
            ['n'] = {
              ['h'] = file_browser_actions.goto_parent_dir,
            }
          }
        }
      },
      pickers = {
        find_files = {
          find_command = { 'rg', '--files', '--hidden', '--glob', '!**/.git/*' },
        }
      }
    })

    telescope.load_extension('fzf')
    telescope.load_extension('file_browser')
  end
}
