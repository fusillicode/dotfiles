local cli = require('nvrim').cli
local keymaps = require('keymaps')
local plugin_keymaps = keymaps.fzf_lua

local no_title = { title = '', }
local with_previewer = {
  previewer = 'builtin',
  winopts   = vim.tbl_extend('error', no_title, {
    height = 0.95,
    preview = {
      default = 'builtin',
      layout = 'vertical',
      vertical = 'down:60%',
    },
  }),
}

return {
  'ibhagwan/fzf-lua',
  keys = plugin_keymaps(),
  dependencies = { { 'junegunn/fzf', build = './install --bin', }, },
  config = function()
    local plugin = require('fzf-lua')

    plugin.setup({
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
        file_icons = false,
        no_header = true,
        no_header_i = true,
      },
      winopts    = {
        title       = '',
        title_flags = false,
        width       = 0.70,
        height      = 0.40,
        row         = 0,
        backdrop    = 100,
        preview     = { default = 'hidden', },
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
          ['<c-k>'] = 'kill-line',
        },
        fzf = {
          ['ctrl-d'] = 'preview-page-down',
          ['ctrl-u'] = 'preview-page-up',
          ['ctrl-q'] = 'select-all+accept',
          ['ctrl-k'] = 'kill-line',
        },
      },
      files      = {
        -- Jump to line! https://github.com/ibhagwan/fzf-lua/discussions/2032#discussioncomment-13046310
        line_query = true,
        winopts    = no_title,
        fzf_opts   = { ['--ansi'] = true, },
        fd_opts    = table.concat(cli.get_fd_flags(), ' '),
        git_icons  = true,
      },
      buffers    = {
        winopts = no_title,
        ignore_current_buffer = true,
      },
      grep       = vim.tbl_extend('error',
        {
          rg_glob        = true,
          rg_opts        = table.concat(cli.get_rg_flags(), ' '),
          hidden         = true,
          glob_flag      = '--iglob',
          glob_separator = '%s%-%-',
        },
        with_previewer
      ),
      git        = {
        status = vim.tbl_extend('error',
          {
            actions = {
              ['ctrl-h'] = { fn = plugin.actions.git_stage, reload = true, },
              ['ctrl-l'] = { fn = plugin.actions.git_unstage, reload = true, },
              ['ctrl-x'] = { fn = plugin.actions.git_reset, reload = true, },
            },
          },
          with_previewer
        ),
      },
    })
    plugin.register_ui_select()
    keymaps.set(plugin_keymaps(plugin))
  end,
}
