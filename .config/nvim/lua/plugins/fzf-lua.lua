local glob_exclusions = {
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
  vim.tbl_map(function(glob) return '--exclude ' .. "'" .. glob .. "'" end, glob_exclusions)
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
  vim.tbl_map(function(glob) return '--glob !' .. "'" .. glob .. "'" end, glob_exclusions)
)

return {
  'ibhagwan/fzf-lua',
  dependencies = { { 'junegunn/fzf', build = './install --bin', }, },
  config = function()
    local fzf_lua = require('fzf-lua')
    local no_title = { title = '', }

    fzf_lua.setup({
      'max-perf',
      fzf_opts   = {
        ['--info'] = 'inline',
        ['--cycle'] = true,
      },
      fzf_colors = {
        ['gutter'] = '-1',
      },
      defaults   = {
        cwd_prompt = false,
        no_header = true,
        no_header_i = true,
      },
      winopts    = {
        title       = '',
        title_flags = false,
        height      = 0.90,
        backdrop    = 100,
        preview     = {
          default = 'builtin',
          layout = 'vertical',
          vertical = 'down:60%',
        },
      },
      keymap     = {
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
      files      = {
        winopts   = no_title,
        fzf_opts  = { ['--ansi'] = true, },
        fd_opts   = table.concat(fd_opts, ' '),
        git_icons = true,
      },
      buffers    = {
        winopts = no_title,
        actions = {
          ['ctrl-x'] = false,
        },
      },
      grep       = {
        winopts        = no_title,
        rg_glob        = true,
        rg_opts        = table.concat(rg_opts, ' '),
        hidden         = true,
        glob_flag      = '--iglob',
        glob_separator = '%s%-%-',
        actions        = {
          ['ctrl-g'] = false,
        },
      },
      git        = {
        status = {
          winopts = no_title,
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
