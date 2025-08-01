local GLOB_EXCLUSIONS = {
  '**/.git/*',
  '**/target/*',
  '**/_build/*',
  '**/deps/*',
  '**/.elixir_ls/*',
  '**/node_modules/*',
}

local fd_opts = vim.list_extend(
  {
    '--color never',
    '--follow',
    '--hidden',
    '--no-ignore-vcs',
    '--type f',
  },
  vim.tbl_map(function(glob) return '--exclude ' .. glob end, GLOB_EXCLUSIONS)
)

local rg_opts = vim.list_extend(
  {
    '--color never',
    '--column',
    '--hidden',
    '--line-number',
    '--no-heading',
    '--smart-case',
    '--with-filename',
  },
  vim.tbl_map(function(glob) return '--glob !' .. glob end, GLOB_EXCLUSIONS)
)

return {
  'ibhagwan/fzf-lua',
  dependencies = { { 'junegunn/fzf', build = './install --bin', }, },
  config = function()
    local fzf_lua = require('fzf-lua')

    fzf_lua.setup({
      'max-perf',
      fzf_opts = {
        ['--info'] = 'inline',
        ['--cycle'] = true,
      },
      defaults = {
        cwd_prompt = false,
        no_header = false,
        no_header_i = false,
      },
      winopts  = {
        title    = '',
        height   = 0.90,
        backdrop = 100,
        preview  = {
          default = 'builtin',
          layout = 'vertical',
          vertical = 'down:60%',
        },
      },
      keymap   = {
        builtin = {
          ['<c-d>'] = 'preview-page-down',
          ['<c-u>'] = 'preview-page-up',
        },
        fzf = {
          ['ctrl-d'] = 'preview-page-down',
          ['ctrl-u'] = 'preview-page-up',
          ['ctrl-q'] = 'select-all+accept',
        },
      },
      files    = {
        winopts   = { title = '', },
        cmd       = 'fd',
        fd_opts   = table.concat(fd_opts, ' '),
        git_icons = true,
      },
      buffers  = {
        winopts = { title = '', },
        actions = {
          ['ctrl-x'] = false,
        },
      },
      grep     = {
        winopts        = { title = '', },
        rg_glob        = true,
        fd_opts        = table.concat(rg_opts, ' '),
        hidden         = true,
        glob_flag      = '--iglob',
        glob_separator = '%s%-%-',
      },
      git      = {
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

    fzf_lua.register_ui_select()

    require('keymaps').fzf_lua(fzf_lua)
  end,
}
