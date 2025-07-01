local colorscheme = require('colorscheme')

local default_sources = {
  'lsp',
  'snippets',
  'buffer',
  'path',
  'cmdline',
  'dictionary',
  'thesaurus',
}

local default_sources_idx = {}
for idx, source in ipairs(default_sources) do
  default_sources_idx[source] = idx
end

return {
  'saghen/blink.cmp',
  event = 'InsertEnter',
  build = 'cargo build --release',
  dependencies = { 'archie-judd/blink-cmp-words', },
  opts = {
    appearance = {
      use_nvim_cmp_as_default = true,
    },
    completion = {
      documentation = {
        auto_show = true,
        auto_show_delay_ms = 0,
        window = colorscheme.window,
      },
      menu = {
        border = colorscheme.window.border,
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
      -- For some reason <c-space> does't work...
      ['<c-x>'] = { 'show', 'show_documentation', 'hide_documentation', },
      ['<c-u>'] = { 'scroll_documentation_up', 'fallback', },
      ['<c-d>'] = { 'scroll_documentation_down', 'fallback', },
    },
    signature = { window = colorscheme.window, },
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
      },
    },
    fuzzy = {
      sorts = {
        function(a, b)
          return (default_sources_idx[b.source_id] or 0) > (default_sources_idx[a.source_id] or 0)
        end,
        'score',
        'sort_text',
      },
    },
  },
}
