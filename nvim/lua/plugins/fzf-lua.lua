return {
  'ibhagwan/fzf-lua',
  keys = { '<leader>', 'gd', 'gr', 'gi', },
  dependencies = { { 'junegunn/fzf', build = './install --bin', }, },
  config = function()
    local keymap_set = require('utils').keymap_set
    keymap_set('n', '<leader>b', function()
      require('fzf-lua').buffers()
    end)
    keymap_set('n', '<leader>f', function()
      require('fzf-lua').files()
    end)

    keymap_set('n', '<leader>gs', function()
      require('fzf-lua').git_status()
    end)
    keymap_set('n', '<leader>gc', function()
      require('fzf-lua').git_commits()
    end)
    keymap_set('n', '<leader>gbc', function()
      require('fzf-lua').git_bcommits()
    end)
    keymap_set('n', '<leader>gb', function()
      require('fzf-lua').git_branches()
    end)

    keymap_set('n', 'gr', function()
      require('fzf-lua').lsp_references({ jump_to_single_result = true, })
    end)
    keymap_set('n', 'gd', function()
      require('fzf-lua').lsp_definitions({ jump_to_single_result = true, })
    end)
    keymap_set('n', 'gi', function()
      require('fzf-lua').lsp_implementations({ jump_to_single_result = true, })
    end)

    keymap_set('n', '<leader>s', function()
      require('fzf-lua').lsp_document_symbols()
    end)
    keymap_set('n', '<leader>S', function()
      require('fzf-lua').lsp_live_workspace_symbols()
    end)
    keymap_set('n', '<leader>a', function()
      require('fzf-lua').lsp_code_actions()
    end)

    keymap_set('n', '<leader>d', function()
      require('fzf-lua').diagnostics_document()
    end)
    keymap_set('n', '<leader>D', function()
      require('fzf-lua').diagnostics_workspace()
    end)

    keymap_set('n', '/', function()
      require('fzf-lua').lgrep_curbuf()
    end)
    keymap_set('n', '<leader>/', function()
      require('fzf-lua').live_grep_glob()
    end)

    keymap_set('n', '<leader>t',
      function()
        require('fzf-lua').grep_curbuf({ search = 'TODO|HACK|PERF|NOTE|FIX', no_esc = true, })
      end)
    keymap_set('n', '<leader>T', function()
      require('fzf-lua').grep({ search = 'TODO|HACK|PERF|NOTE|FIX', no_esc = true, })
    end)

    keymap_set('n', '<leader>l', function()
      require('fzf-lua').resume()
    end)

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
