return {
  'ibhagwan/fzf-lua',
  keys = { '<leader>', 'gd', 'gr', 'gi', },
  dependencies = { { 'junegunn/fzf', build = './install --bin', }, },
  config = function()
    local fzf_lua = require('fzf-lua')
    local keymap_set = require('utils').keymap_set

    keymap_set('n', '<leader>b', function()
      fzf_lua.buffers()
    end)
    keymap_set('n', '<leader>f', function()
      fzf_lua.files()
    end)

    keymap_set('n', '<leader>gs', function()
      fzf_lua.git_status()
    end)
    keymap_set('n', '<leader>gc', function()
      fzf_lua.git_commits()
    end)
    keymap_set('n', '<leader>gbc', function()
      fzf_lua.git_bcommits()
    end)
    keymap_set('n', '<leader>gb', function()
      fzf_lua.git_branches()
    end)

    keymap_set('n', 'gr', function()
      fzf_lua.lsp_references({ jump_to_single_result = true, })
    end)
    keymap_set('n', 'gd', function()
      fzf_lua.lsp_definitions({ jump_to_single_result = true, })
    end)
    keymap_set('n', 'gi', function()
      fzf_lua.lsp_implementations({ jump_to_single_result = true, })
    end)

    keymap_set('n', '<leader>s', function()
      fzf_lua.lsp_document_symbols()
    end)
    keymap_set('n', '<leader>S', function()
      fzf_lua.lsp_live_workspace_symbols()
    end)
    keymap_set('n', '<leader>a', function()
      fzf_lua.lsp_code_actions()
    end)

    keymap_set('n', '<leader>d', function()
      fzf_lua.diagnostics_document()
    end)
    keymap_set('n', '<leader>D', function()
      fzf_lua.diagnostics_workspace()
    end)

    keymap_set('n', '/', function()
      fzf_lua.lgrep_curbuf({ continue_last_search = true, })
    end)
    keymap_set('n', '<leader>/', function()
      fzf_lua.live_grep_glob({ continue_last_search = true, })
    end)

    keymap_set('n', '<leader>t',
      function()
        fzf_lua.grep_curbuf({ search = 'TODO:|HACK:|PERF:|NOTE:|FIX:|WARN:', no_esc = true, })
      end)
    keymap_set('n', '<leader>T', function()
      fzf_lua.grep({ search = 'TODO:|HACK:|PERF:|NOTE:|FIX:|WARN:', no_esc = true, })
    end)

    keymap_set('n', '<leader>l', function()
      fzf_lua.resume()
    end)

    fzf_lua.setup({
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
