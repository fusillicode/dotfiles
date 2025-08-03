return {
  'ibhagwan/fzf-lua',
  keys = { '<leader>', 'gd', 'gr', 'gi', },
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
        ['prompt']    = { 'fg', 'Special', },
        ['info']      = { 'fg', 'Special', },
        ['fg']        = { 'fg', 'Comment', },
        ['hl']        = { 'fg', 'Special', },
        ['hl+']       = { 'fg', 'Special', },
        ['pointer']   = { 'bg', 'Normal', },
        ['scrollbar'] = { 'fg', 'Normal', },
        ['gutter']    = '-1',
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
      previewers = {
        builtin = {
          title_fnamemodify = function(s)
            return vim.fn.fnamemodify(s, ':.')
          end,
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
        fd_opts   = table.concat(
          require('rua').get_fd_cli_flags(),
          ' '
        ),
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
        rg_opts        = table.concat(
          require('rua').get_rg_cli_flags(),
          ' '
        ),
        hidden         = true,
        glob_flag      = '--iglob',
        glob_separator = '%s%-%-',
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
