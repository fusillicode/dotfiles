return {
  'ibhagwan/fzf-lua',
  keys = { '<leader>', 'gd', 'gr', 'gi', },
  dependencies = { { 'junegunn/fzf', build = './install --bin', }, },
  config = function()
    local fzf_lua = require('fzf-lua')
    local keymap_set = require('utils').keymap_set

    keymap_set('n', '<leader>f', function() fzf_lua.files({ prompt = 'Files: ', }) end)
    keymap_set('n', '<leader>b', function() fzf_lua.buffers({ prompt = 'Buffers: ', }) end)

    keymap_set('n', '<leader>gs', function() fzf_lua.git_status({ prompt = 'Changes: ', }) end)
    keymap_set('n', '<leader>gc', function() fzf_lua.git_commits({ prompt = 'Commits: ', }) end)
    keymap_set('n', '<leader>gcc', function() fzf_lua.git_bcommits({ prompt = 'Buffer commits: ', }) end)
    keymap_set('n', '<leader>gb', function() fzf_lua.git_branches({ prompt = 'Branches: ', }) end)

    local lsp_jumps_cfg = { ignore_current_line = true, jump_to_single_result = true, }
    keymap_set('n', 'gr', function()
      fzf_lua.lsp_references(vim.tbl_extend('error', { prompt = 'References: ', }, lsp_jumps_cfg))
    end)
    keymap_set('n', 'gd', function()
      fzf_lua.lsp_definitions(vim.tbl_extend('error', { prompt = 'Definitions: ', }, lsp_jumps_cfg))
    end)
    keymap_set('n', 'gi', function()
      fzf_lua.lsp_implementations(vim.tbl_extend('error', { prompt = 'Implementations: ', }, lsp_jumps_cfg))
    end)

    keymap_set('n', '<leader>s', function() fzf_lua.lsp_document_symbols({ prompt = 'Buffer symbols: ', }) end)
    keymap_set('n', '<leader>S', function() fzf_lua.lsp_live_workspace_symbols({ prompt = 'Workspace symbols: ', }) end)
    keymap_set('n', '<leader>a', function() fzf_lua.lsp_code_actions({ prompt = 'Code actions: ', }) end)

    keymap_set('n', '<leader>d', function() fzf_lua.diagnostics_document({ prompt = 'Buffer aiagnostics: ', }) end)
    keymap_set('n', '<leader>D', function() fzf_lua.diagnostics_workspace({ prompt = 'Workspace diagnostics: ', }) end)

    keymap_set('n', '<leader>/', function()
      fzf_lua.live_grep_glob({ continue_last_search = true, prompt = 'rg: ', })
    end)

    local todo_comments_cfg = { search = 'TODO:|HACK:|PERF:|NOTE:|FIX:|FIXME:|WARN:', no_esc = true, }
    keymap_set('n', '<leader>t', function()
      fzf_lua.grep_curbuf(vim.tbl_extend('error', todo_comments_cfg, { prompt = 'Buffer TODOs: ', }))
    end)
    keymap_set('n', '<leader>T', function()
      fzf_lua.grep(vim.tbl_extend('error', todo_comments_cfg, { prompt = 'Workspace TODOs: ', }))
    end)

    keymap_set('n', '<leader>l', function() fzf_lua.resume() end)

    fzf_lua.setup({
      fzf_colors = {
        ['fg']      = { 'fg', 'LineNr', },
        ['bg']      = { 'bg', 'Normal', },
        ['hl']      = { 'fg', 'TelescopeMatching', },
        ['fg+']     = { 'fg', 'Normal', },
        ['bg+']     = { 'bg', 'CursorLine', },
        ['hl+']     = { 'fg', 'TelescopeMatching', },
        ['info']    = { 'fg', 'Keyword', },
        ['prompt']  = { 'fg', 'Keyword', },
        ['pointer'] = { 'bg', 'CursorLine', },
        ['marker']  = { 'fg', 'Keyword', },
        ['spinner'] = { 'fg', 'Label', },
        ['header']  = { 'fg', 'Comment', },
        ['gutter']  = { 'bg', 'Normal', },
      },
      winopts = {
        preview = {
          default = 'builtin',
          layout = 'vertical',
          delay = 0,
          title = false,
        },
      },
      defaults = {
        file_icons = false,
        git_icons = false,
        cwd_prompt = false,
        fzf_opts = {
          ['--cycle'] = '',
          ['--info'] = 'inline',
          ['--no-header'] = '',
        },
      },
      keymap = {
        builtin = {
          ['<c-d>'] = 'preview-page-down',
          ['<c-u>'] = 'preview-page-up',
        },
      },
    })
  end,
}
