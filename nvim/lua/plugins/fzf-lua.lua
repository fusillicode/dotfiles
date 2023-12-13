return {
  'ibhagwan/fzf-lua',
  keys = { '<leader>', 'gd', 'gr', 'gi', },
  dependencies = { { 'junegunn/fzf', build = './install --bin', }, },
  config = function()
    local fzf_lua = require('fzf-lua')
    local keymap_set = require('utils').keymap_set

    keymap_set('n', '<leader>b', function() fzf_lua.buffers() end)
    keymap_set('n', '<leader>f', function() fzf_lua.files() end)

    keymap_set('n', '<leader>gs', function() fzf_lua.git_status() end)
    keymap_set('n', '<leader>gc', function() fzf_lua.git_commits() end)
    keymap_set('n', '<leader>gfc', function() fzf_lua.git_bcommits({ prompt = 'Buffer Commits>', }) end)
    keymap_set('n', '<leader>gb', function() fzf_lua.git_branches() end)

    local lsp_jumps_cfg = { ignore_current_line = true, jump_to_single_result = true, }
    keymap_set('n', 'gr', function() fzf_lua.lsp_references(lsp_jumps_cfg) end)
    keymap_set('n', 'gd', function() fzf_lua.lsp_definitions(lsp_jumps_cfg) end)
    keymap_set('n', 'gi', function() fzf_lua.lsp_implementations(lsp_jumps_cfg) end)

    keymap_set('n', '<leader>s', function() fzf_lua.lsp_document_symbols() end)
    keymap_set('n', '<leader>S', function() fzf_lua.lsp_live_workspace_symbols() end)
    keymap_set('n', '<leader>a', function() fzf_lua.lsp_code_actions() end)

    keymap_set('n', '<leader>d', function() fzf_lua.diagnostics_document({ prompt = 'Buffer Diagnostics>', }) end)
    keymap_set('n', '<leader>D', function() fzf_lua.diagnostics_workspace({ prompt = 'Workspace Diagnostics>', }) end)

    local live_grep_cfg = { continue_last_search = true, }
    keymap_set('n', '/', function()
      fzf_lua.lgrep_curbuf(vim.tbl_extend('error', live_grep_cfg, { prompt = 'Buffer rg> ', }))
    end)
    keymap_set('n', '<leader>/', function()
      fzf_lua.live_grep_glob(vim.tbl_extend('error', live_grep_cfg, { prompt = 'Workspace rg> ', }))
    end)

    local todo_comments_cfg = { search = 'TODO:|HACK:|PERF:|NOTE:|FIX:|WARN:', no_esc = true, }
    keymap_set('n', '<leader>t', function()
      fzf_lua.grep_curbuf(vim.tbl_extend('error', todo_comments_cfg, { prompt = 'Buffer TODOs> ', }))
    end)
    keymap_set('n', '<leader>T', function()
      fzf_lua.grep(vim.tbl_extend('error', todo_comments_cfg, { prompt = 'Workspace TODOs> ', }))
    end)

    keymap_set('n', '<leader>l', function() fzf_lua.resume() end)

    fzf_lua.setup({
      'max-perf',
      fzf_opts = { ['--cycle'] = '', },
      fzf_colors = {
        ['fg']      = { 'fg', 'CursorLine', },
        ['bg']      = { 'bg', 'Normal', },
        ['hl']      = { 'fg', 'TelescopeMatching', },
        ['fg+']     = { 'fg', 'Normal', },
        ['bg+']     = { 'bg', 'CursorLine', },
        ['hl+']     = { 'fg', 'TelescopeMatching', },
        ['info']    = { 'fg', 'LineNr', },
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
        },
      },
      files = {
        prompt = 'Files> ',
        cwd_prompt = false,
      },
    })
  end,
}
