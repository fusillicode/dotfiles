-- local GLOB_EXCLUSIONS = {
--   '**/.git/*',
--   '**/target/*',
--   '**/_build/*',
--   '**/deps/*',
--   '**/.elixir_ls/*',
--   '**/node_modules/*',
-- }
-- local find_command = vim.list_extend(
--   {
--     'fd',
--     '--color=never',
--     '--type=f',
--     '--follow',
--     '--no-ignore-vcs',
--     '--hidden',
--   },
--   vim.tbl_map(function(glob) return '--exclude=' .. glob end, GLOB_EXCLUSIONS))
-- local vimgrep_arguments = vim.list_extend(
--   {
--     'rg',
--     '--color=never',
--     '--no-heading',
--     '--with-filename',
--     '--line-number',
--     '--column',
--     '--smart-case',
--     '--hidden',
--   },
--   vim.tbl_map(function(glob) return '--glob=!' .. glob end, GLOB_EXCLUSIONS)
-- )

return {
  'ibhagwan/fzf-lua',
  config = function()
    local fzf_lua = require('fzf-lua')

    fzf_lua.setup({
      'max-perf',
      fzf_opts = {
        ['--info'] = 'inline-right',
        ['--cycle'] = true,
      },
      defaults = {
        cwd_prompt = false,
        no_header = false,
        no_header_i = false,
      },
      winopts = {
        title    = '',
        height   = 0.90,
        backdrop = 100,
        preview  = {
          default = 'builtin',
          layout = 'vertical',
          vertical = 'down:60%',
        },
      },
      keymap = {
        builtin = {
          ['<c-d>'] = 'preview-page-down',
          ['<c-u>'] = 'preview-page-up',
        },
        fzf = {
          ['ctrl-d'] = 'preview-page-down',
          ['ctrl-u'] = 'preview-page-up',
        },
      },
      files = {
        winopts = { title = '', },
        git_icons = true,
      },
      buffers = {
        winopts = { title = '', },
        actions = {
          ['ctrl-x'] = false,
        },
      },
      git = {
        status = {
          winopts = { title = '', },
          actions = {
            ['right']  = false,
            ['left']   = false,
            ['ctrl-x'] = false,
            ['ctrl-s'] = false,
          },
        },
      },
    })

    require('keymaps').fzf_lua(fzf_lua)
  end,
}
