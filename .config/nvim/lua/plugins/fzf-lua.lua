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
          ['ctrl-n'] = 'preview-page-down',
          ['ctrl-p'] = 'preview-page-up',
          ['ctrl-f'] = 'select-all+accept',
        },
      },
      winopts = {
        preview = {
          default = 'builtin',
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
        -- GIGACHAD ✨ https://www.reddit.com/r/neovim/comments/1c1id24/comment/kz3jnkc/
        fzf_opts = {
          ['--ansi'] = '',
          ['--with-nth'] = '2..',
          ['--delimiter'] = '\\s',
          ['--tiebreak'] = 'begin,index',
        },
        cmd = string.format(
          'fd --color=never --type f --hidden --follow --no-ignore-vcs ' ..
          '--exclude .git --exclude target --exclude node_modules --exclude _build --exclude deps --exclude .elixir_ls ' ..
          '-x echo {} | awk -F / ' ..
          [['{printf "%%s: ", $0; printf "%%s ", $NF; gsub(/^\.\//, "", $0); gsub($NF, "", $0); printf "%s ", $0; print ""}']],
          require('fzf-lua.utils').ansi_codes.grey('%s')
        ),
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
          child_prefix = false,
        },
      },
    })
  end,
}
