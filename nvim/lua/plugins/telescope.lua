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

    vkeyset('n', '<leader>b', telescope_builtin.buffers)
    vkeyset('n', '<leader>f', telescope_builtin.find_files)
    vkeyset('n', '<leader>j', telescope_builtin.jumplist)
    vkeyset('n', '<leader>F', ':Telescope file_browser path=%:p:h select_buffer=true<CR>')
    vkeyset('n', '<leader>t', ':TodoTelescope<CR>')
    vkeyset('n', '<leader>l', telescope.extensions.live_grep_args.live_grep_args)
    vkeyset('n', '<leader>c', telescope_builtin.git_commits)
    vkeyset('n', '<leader>bc', telescope_builtin.git_bcommits)
    vkeyset('n', '<leader>gb', telescope_builtin.git_branches)
    vkeyset('n', '<leader>gs', telescope_builtin.git_status)
    vkeyset('n', '<leader>d', function() telescope_builtin.diagnostics({ bufnr = 0 }) end)
    vkeyset('n', '<leader>D', telescope_builtin.diagnostics)
    vkeyset('n', '<leader>s', telescope_builtin.lsp_document_symbols)
    vkeyset('n', '<leader>S', telescope_builtin.lsp_dynamic_workspace_symbols)
    vkeyset('n', 'gd', telescope_builtin.lsp_definitions)
    vkeyset('n', 'gr', telescope_builtin.lsp_references)
    vkeyset('n', 'gi', telescope_builtin.lsp_implementations)

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
