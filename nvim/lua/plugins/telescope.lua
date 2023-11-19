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
    local custom_preview = {
      show_line = false,
      results_title = false,
      preview_title = false,
    }

    vkeyset('n', '<leader>b', function() telescope_builtin.buffers(custom_preview) end)
    vkeyset('n', '<leader>f', function() telescope_builtin.find_files(custom_preview) end)
    vkeyset('n', '<leader>j', function() telescope_builtin.jumplist(custom_preview) end)
    vkeyset('n', '<leader>c', function() telescope_builtin.git_commits(custom_preview) end)
    vkeyset('n', '<leader>bc', function() telescope_builtin.git_bcommits(custom_preview) end)
    vkeyset('n', '<leader>gb', function() telescope_builtin.git_branches(custom_preview) end)
    vkeyset('n', '<leader>gs', function() telescope_builtin.git_status(custom_preview) end)
    vkeyset('n', '<leader>d', function() telescope_builtin.diagnostics({ bufnr = 0 }) end)
    vkeyset('n', '<leader>D', function() telescope_builtin.diagnostics(custom_preview) end)
    vkeyset('n', '<leader>s', function() telescope_builtin.lsp_document_symbols(custom_preview) end)
    vkeyset('n', '<leader>S', function() telescope_builtin.lsp_dynamic_workspace_symbols(custom_preview) end)
    vkeyset('n', 'gd', function() telescope_builtin.lsp_definitions(custom_preview) end)
    vkeyset('n', 'gr', function() telescope_builtin.lsp_references(custom_preview) end)
    vkeyset('n', 'gi', function() telescope_builtin.lsp_implementations(custom_preview) end)
    vkeyset('n', '<leader>l', function() telescope.extensions.live_grep_args.live_grep_args(custom_preview) end)
    vkeyset('n', '<leader>F', ':Telescope file_browser path=%:p:h select_buffer=true<CR>')
    vkeyset('n', '<leader>t', ':TodoTelescope<CR>')

    local file_browser_actions = require('telescope._extensions.file_browser.actions')
    telescope.setup({
      defaults = { layout_strategy = 'vertical' },
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
