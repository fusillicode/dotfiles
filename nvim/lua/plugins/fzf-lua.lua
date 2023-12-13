return {
  'ibhagwan/fzf-lua',
  keys = { '<leader>', 'gd', 'gr', 'gi', },
  dependencies = { { 'junegunn/fzf', build = './install --bin', }, },
  config = function()
    local keymap_set = require('utils').keymap_set
    keymap_set('n', '<leader>b', ":lua require('fzf-lua').buffers()<cr>")
    keymap_set('n', '<leader>f', ":lua require('fzf-lua').files()<cr>")

    keymap_set('n', '<leader>gs', ":lua require('fzf-lua').git_status()<cr>")
    keymap_set('n', '<leader>gc', ":lua require('fzf-lua').git_commits()<cr>")
    keymap_set('n', '<leader>gbc', ":lua require('fzf-lua').git_bcommits()<cr>")
    keymap_set('n', '<leader>gb', ":lua require('fzf-lua').git_branches()<cr>")

    keymap_set('n', 'gr', ":lua require('fzf-lua').lsp_references({jump_to_single_result=true})<cr>")
    keymap_set('n', 'gd', ":lua require('fzf-lua').lsp_definitions({jump_to_single_result=true})<cr>")
    keymap_set('n', 'gi', ":lua require('fzf-lua').lsp_implementations({jump_to_single_result=true})<cr>")

    keymap_set('n', '<leader>s', ":lua require('fzf-lua').lsp_document_symbols()<cr>")
    keymap_set('n', '<leader>S', ":lua require('fzf-lua').lsp_live_workspace_symbols()<cr>")
    keymap_set('n', '<leader>a', ":lua require('fzf-lua').lsp_code_actions()<cr>")

    keymap_set('n', '<leader>d', ":lua require('fzf-lua').diagnostics_document()<cr>")
    keymap_set('n', '<leader>D', ":lua require('fzf-lua').diagnostics_workspace()<cr>")

    keymap_set('n', '/', ":lua require('fzf-lua').lgrep_curbuf()<cr>")
    keymap_set('n', '<leader>/', ":lua require('fzf-lua').live_grep_glob()<cr>")
    keymap_set('n', '<leader>t',
      ":lua require('fzf-lua').grep_curbuf({search='TODO|HACK|PERF|NOTE|FIX', no_esc=true})<cr>")
    keymap_set('n', '<leader>T', ":lua require('fzf-lua').grep({search='TODO|HACK|PERF|NOTE|FIX', no_esc=true})<cr>")

    keymap_set('n', '<leader>l', ":lua require('fzf-lua').resume()<cr>")

    require('fzf-lua').setup({
      'max-perf',
      fzf_opts = { ['--cycle'] = '', },
      winopts = {
        preview = {
          default = 'builtin',
          layout = 'vertical',
          delay = 0,
        },
      },
      files = {
        cwd_prompt = false,
      },
    })
  end,
}
