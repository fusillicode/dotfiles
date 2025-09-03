local style_opts = require('rua').get_style_opts()
local get_item_idx = require('utils').item_idx

local default_sources = {
  'lsp',
  'cmp_dbee',
  'snippets',
  'buffer',
  'path',
  'cmdline',
  'dictionary',
  'thesaurus',
}

return {
  'saghen/blink.cmp',
  event = 'InsertEnter',
  build = 'cargo build --release',
  dependencies = {
    'archie-judd/blink-cmp-words',
    { 'saghen/blink.compat', version = '2.*', lazy = true, opts = {}, },
    {
      'MattiasMTS/cmp-dbee',
      ft = 'sql',
      opts = {},
    },
  },
  opts = {
    appearance = {
      use_nvim_cmp_as_default = true,
    },
    completion = {
      documentation = {
        auto_show = true,
        auto_show_delay_ms = 0,
        window = style_opts.window,
      },
      menu = {
        border = style_opts.window.border,
        draw = {
          columns = {
            { 'source_name', gap = 1, 'label', },
          },
          components = {
            source_name = {
              text = function(ctx) return '[' .. ctx.source_name .. ']' end,
            },
          },
        },
      },
    },
    keymap = {
      preset = 'enter',
      -- For some reason <c-space> doesn't work...
      ['<c-x>'] = { 'show', 'show_documentation', 'hide_documentation', },
      ['<c-u>'] = { 'scroll_documentation_up', 'fallback', },
      ['<c-d>'] = { 'scroll_documentation_down', 'fallback', },
    },
    signature = { window = style_opts.window, },
    sources = {
      default = default_sources,
      providers = {
        snippets = {
          opts = {
            search_paths = {
              vim.fn.expand('~') .. '/data/dev/dotfiles/dotfiles/vscode/snippets',
            },
          },
        },
        thesaurus = {
          name = 'blink-cmp-words',
          module = 'blink-cmp-words.thesaurus',
        },
        dictionary = {
          name = 'blink-cmp-words',
          module = 'blink-cmp-words.dictionary',
        },
        cmp_dbee = {
          name = 'cmp-dbee',
          module = 'blink.compat.source',
        },
      },
    },
    fuzzy = {
      sorts = {
        function(a, b)
          return
              (get_item_idx(default_sources, b.source_id) or 0) >
              (get_item_idx(default_sources, a.source_id) or 0)
        end,
        'score',
        'sort_text',
      },
    },
  },
}
