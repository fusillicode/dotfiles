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
        no_header = true,
        fzf_opts = {
          ['--cycle'] = '',
          ['--info'] = 'inline',
        },
      },
      fzf_colors = {
        ['fg']      = { 'fg', 'StatusLine', },
        ['bg']      = { 'bg', 'Normal', },
        ['hl']      = { 'bg', 'IncSearch', },
        ['fg+']     = { 'fg', 'Normal', },
        ['bg+']     = { 'bg', 'CursorLine', },
        ['hl+']     = { 'bg', 'IncSearch', },
        ['info']    = { 'fg', 'Keyword', },
        ['prompt']  = { 'bg', 'IncSearch', },
        ['pointer'] = { 'bg', 'CursorLine', },
        ['marker']  = { 'fg', 'Keyword', },
        ['spinner'] = { 'fg', 'Label', },
        ['header']  = { 'fg', 'Comment', },
        ['gutter']  = { 'bg', 'Normal', },
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
      winopts = {
        preview = {
          default = 'builtin',
          layout = 'vertical',
          title = true,
          title_pos = 'left',
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
        fd_opts = '--color=never --type f --hidden --follow --exclude .git ' ..
            '--no-ignore-vcs --exclude target --exclude node_modules',
      },
      git = {
        branches = {
          cmd = 'git branch --all --color --sort=-committerdate',
        },
      },
      grep = {
        rg_glob = true,
        rg_opts = '--column --line-number --no-heading --color=always --smart-case --max-columns=4096 ' ..
            '--hidden --glob "!.git/*" --glob "!node_modules/*" --glob "!target/*" -e',
      },
      lsp = {
        symbols = {
          symbol_icons = {
            File          = 'file',
            Module        = 'mod',
            Namespace     = 'namespace',
            Package       = 'package',
            Class         = 'class',
            Method        = 'method',
            Property      = 'prop',
            Field         = 'field',
            Constructor   = 'constructor',
            Enum          = 'enum',
            Interface     = 'interf',
            Function      = 'fn',
            Variable      = 'var',
            Constant      = 'const',
            String        = 'str',
            Number        = 'num',
            Boolean       = 'bool',
            Array         = 'array',
            Object        = 'obj',
            Key           = 'key',
            Null          = 'null',
            EnumMember    = 'variant',
            Struct        = 'struct',
            Event         = 'event',
            Operator      = 'operator',
            TypeParameter = 'type',
          },
          symbol_fmt   = function(s, opts)
            return opts['symbol_icons'][s] and (opts['symbol_icons'][s] .. ':')
          end,
          child_prefix = false,
        },
      },
    })
  end,
}
