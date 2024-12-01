return {
  'ibhagwan/fzf-lua',
  keys = { '<leader>', 'gd', 'gr', 'gi', },
  dependencies = { { 'junegunn/fzf', build = './install --bin', }, },
  config = function()
    local fzf_lua = require('fzf-lua')
    require('keymaps').fzf_lua(fzf_lua)

    fzf_lua.setup({
      'max-perf',
      defaults = {
        cwd_prompt = false,
        no_header  = true,
        fzf_opts   = {
          ['--cycle'] = '',
          ['--info'] = 'inline',
        },
      },
      keymap = {
        builtin = {
          ['<c-d>'] = 'preview-page-down',
          ['<c-u>'] = 'preview-page-up',
        },
        fzf = {
          ['ctrl-n'] = 'preview-page-down',
          ['ctrl-p'] = 'preview-page-up',
          ['ctrl-q'] = 'select-all+accept',
        },
      },
      winopts = {
        height = '0.9',
        preview = {
          default = 'builtin',
          title = true,
          title_pos = 'center',
          layout = 'vertical',
          vertical = 'down:60%',
        },
      },
      previewers = {
        builtin = {
          limit_b = 1200000,
        },
      },
      commands = {
        sort_lastused = true,
      },
      diagnostics = {
        signs = {
          ['Error'] = { text = 'E', texthl = 'DiagnosticError', },
          ['Warn']  = { text = 'W', texthl = 'DiagnosticWarn', },
          ['Info']  = { text = 'I', texthl = 'DiagnosticInfo', },
          ['Hint']  = { text = 'H', texthl = 'DiagnosticHint', },
        },
      },
      files = {
        fd_opts = '--color=never --type f --hidden --follow --no-ignore-vcs ' ..
            table.concat(require('utils').FD_EXCLUSIONS, ' '),
      },
      git = {
        branches = {
          cmd = 'git branch --all --color --sort=-committerdate',
        },
      },
      grep = {
        rg_glob = true,
        rg_opts = '--no-ignore-vcs --column --line-number --no-heading --smart-case --max-columns=4096 ' ..
            table.concat(require('utils').RG_EXCLUSIONS, ' '),
      },
      lsp = {
        symbols = {
          child_prefix = false,
        },
      },
    })
  end,
}
