return {
  'ibhagwan/fzf-lua',
  keys = { '<leader>', 'gd', 'gr', 'gi', },
  dependencies = { { 'junegunn/fzf', build = './install --bin', }, },
  config = function()
    local keymap_set = require('utils').keymap_set
    keymap_set('n', '<leader>b', ":lua require('fzf-lua').buffers()<CR>")
    keymap_set('n', '<leader>f', ":lua require('fzf-lua').files()<CR>")

    keymap_set('n', '<leader>gs', ":lua require('fzf-lua').git_status()<CR>")
    keymap_set('n', '<leader>gc', ":lua require('fzf-lua').git_commits()<CR>")
    keymap_set('n', '<leader>gbc', ":lua require('fzf-lua').git_bcommits()<CR>")
    keymap_set('n', '<leader>gb', ":lua require('fzf-lua').git_branches()<CR>")

    keymap_set('n', 'gr', ":lua require('fzf-lua').lsp_references({jump_to_single_result=true})<CR>")
    keymap_set('n', 'gd', ":lua require('fzf-lua').lsp_definitions({jump_to_single_result=true})<CR>")
    keymap_set('n', 'gi', ":lua require('fzf-lua').lsp_implementations({jump_to_single_result=true})<CR>")

    keymap_set('n', '<leader>s', ":lua require('fzf-lua').lsp_document_symbols()<CR>")
    keymap_set('n', '<leader>S', ":lua require('fzf-lua').lsp_live_workspace_symbols()<CR>")
    keymap_set('n', '<leader>a', ":lua require('fzf-lua').lsp_code_actions()<CR>")

    keymap_set('n', '<leader>d', ":lua require('fzf-lua').diagnostics_document()<CR>")
    keymap_set('n', '<leader>D', ":lua require('fzf-lua').diagnostics_workspace()<CR>")

    keymap_set('n', '/', ":lua require('fzf-lua').lgrep_curbuf()<CR>")
    keymap_set('n', '<leader>/', ":lua require('fzf-lua').live_grep_glob()<CR>")
    keymap_set('n', '<leader>t',
      ":lua require('fzf-lua').grep_curbuf({search='TODO|HACK|PERF|NOTE|FIX', no_esc=true})<CR>")
    keymap_set('n', '<leader>T', ":lua require('fzf-lua').grep({search='TODO|HACK|PERF|NOTE|FIX', no_esc=true})<CR>")

    keymap_set('n', '<leader>l', ":lua require('fzf-lua').resume()<CR>")

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
